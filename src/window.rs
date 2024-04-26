use std::{cell::Cell, rc::Rc};

use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use servo::{
    compositing::windowing::{EmbedderEvent, MouseWindowEvent, WindowMethods},
    embedder_traits::{Cursor, EmbedderMsg},
    euclid::{Point2D, Size2D, UnknownUnit},
    gl,
    msg::constellation_msg::WebViewId,
    rendering_context::RenderingContext,
    script_traits::{TouchEventType, WheelDelta, WheelMode},
    webrender_api::{
        units::{DeviceIntPoint, DevicePoint, LayoutVector2D},
        ScrollLocation,
    },
    Servo,
};
use surfman::{Connection, GLApi, SurfaceType};
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, TouchPhase, WindowEvent},
    window::{CursorIcon, Window as WinitWindow},
};

use crate::{webview::WebView, Status};

/// A Verso window is a Winit window containing several web views.
pub struct Window {
    /// Access to winit window
    window: Rc<WinitWindow>,
    /// The web view ID obtained from initialized servo
    webview_id: Option<WebViewId>,
    /// The web view this window owns.
    webview: Rc<WebView>,
    /// Access to webrender rendering context
    rendering_context: RenderingContext,
    /// Access to webrender gl
    webrender_gl: Rc<dyn gl::Gl>,
    /// The mouse physical position in the web view.
    mouse_position: Cell<PhysicalPosition<f64>>,
}

impl Window {
    /// Create a Verso window from winit window.
    pub fn new(window: WinitWindow) -> Self {
        let window_size = window.inner_size();
        let window_size = Size2D::new(window_size.width as i32, window_size.height as i32);
        let display_handle = window.raw_display_handle();
        let connection = Connection::from_raw_display_handle(display_handle)
            .expect("Failed to create connection");
        let adapter = connection
            .create_adapter()
            .expect("Failed to create adapter");
        let native_widget = connection
            .create_native_widget_from_raw_window_handle(window.raw_window_handle(), window_size)
            .expect("Failed to create native widget");
        let surface_type = SurfaceType::Widget { native_widget };
        let rendering_context = RenderingContext::create(&connection, &adapter, surface_type)
            .expect("Failed to create rendering context");
        log::trace!("Created rendering context for window {:?}", window);
        let webrender_gl = match rendering_context.connection().gl_api() {
            GLApi::GL => unsafe { gl::GlFns::load_with(|s| rendering_context.get_proc_address(s)) },
            GLApi::GLES => unsafe {
                gl::GlesFns::load_with(|s| rendering_context.get_proc_address(s))
            },
        };
        debug_assert_eq!(webrender_gl.get_error(), gl::NO_ERROR);

        let window = Rc::new(window);
        let webview = Rc::new(WebView::new(window.clone(), rendering_context.clone()));
        Self {
            rendering_context,
            window,
            webview_id: None,
            webview,
            webrender_gl,
            mouse_position: Cell::new(PhysicalPosition::default()),
        }
    }

    /// Set web view ID of this window.
    pub fn set_webview_id(&mut self, id: WebViewId) {
        self.webview_id = Some(id);
    }

    /// Return the window's web view.
    pub fn webview(&self) -> Rc<WebView> {
        return self.webview.clone();
    }

    /// Handle winit window event.
    pub fn handle_winit_window_event(
        &self,
        servo: &mut Option<Servo<WebView>>,
        events: &mut Vec<EmbedderEvent>,
        event: &winit::event::WindowEvent,
    ) {
        match event {
            WindowEvent::RedrawRequested => {
                let Some(servo) = servo.as_mut() else {
                    return;
                };

                self.paint(servo);
                events.push(EmbedderEvent::Idle);
            }
            WindowEvent::Resized(size) => {
                let size = Size2D::new(size.width, size.height);
                let _ = self.resize(size.to_i32());
                events.push(EmbedderEvent::WindowResize);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let event: DevicePoint = DevicePoint::new(position.x as f32, position.y as f32);
                self.mouse_position.set(*position);
                events.push(EmbedderEvent::MouseWindowMoveEventClass(event));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button: servo::script_traits::MouseButton = match button {
                    winit::event::MouseButton::Left => servo::script_traits::MouseButton::Left,
                    winit::event::MouseButton::Right => servo::script_traits::MouseButton::Right,
                    winit::event::MouseButton::Middle => servo::script_traits::MouseButton::Middle,
                    _ => {
                        log::warn!(
                            "Verso Window hasn't supported this mouse button yet: {button:?}"
                        );
                        return;
                    }
                };
                let position = Point2D::new(
                    self.mouse_position.get().x as f32,
                    self.mouse_position.get().y as f32,
                );

                let event: MouseWindowEvent = match state {
                    ElementState::Pressed => MouseWindowEvent::MouseDown(button, position),
                    ElementState::Released => MouseWindowEvent::MouseUp(button, position),
                };
                events.push(EmbedderEvent::MouseWindowEventClass(event));

                // winit didn't send click event, so we send it after mouse up
                if *state == ElementState::Released {
                    let event: MouseWindowEvent = MouseWindowEvent::Click(button, position);
                    events.push(EmbedderEvent::MouseWindowEventClass(event));
                }
            }
            WindowEvent::TouchpadMagnify { delta, .. } => {
                events.push(EmbedderEvent::Zoom(1.0 + *delta as f32));
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                // FIXME: Pixels per line, should be configurable (from browser setting?) and vary by zoom level.
                const LINE_HEIGHT: f32 = 38.0;

                let (mut x, mut y, mode) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        (*x as f64, (*y * LINE_HEIGHT) as f64, WheelMode::DeltaLine)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(position) => {
                        let position = position.to_logical::<f64>(self.window.scale_factor());
                        (position.x, position.y, WheelMode::DeltaPixel)
                    }
                };

                // Wheel Event
                events.push(EmbedderEvent::Wheel(
                    WheelDelta { x, y, z: 0.0, mode },
                    DevicePoint::new(
                        self.mouse_position.get().x as f32,
                        self.mouse_position.get().y as f32,
                    ),
                ));

                // Scroll Event
                // Do one axis at a time.
                if y.abs() >= x.abs() {
                    x = 0.0;
                } else {
                    y = 0.0;
                }

                let phase: TouchEventType = match phase {
                    TouchPhase::Started => TouchEventType::Down,
                    TouchPhase::Moved => TouchEventType::Move,
                    TouchPhase::Ended => TouchEventType::Up,
                    TouchPhase::Cancelled => TouchEventType::Cancel,
                };

                events.push(EmbedderEvent::Scroll(
                    ScrollLocation::Delta(LayoutVector2D::new(x as f32, y as f32)),
                    DeviceIntPoint::new(
                        self.mouse_position.get().x as i32,
                        self.mouse_position.get().y as i32,
                    ),
                    phase,
                ));
            }
            WindowEvent::CloseRequested => {
                events.push(EmbedderEvent::Quit);
            }
            e => log::warn!("Verso Window hasn't supported this window event yet: {e:?}"),
        }
    }

    /// Handle servo messages and return a boolean to indicate servo needs to present or not.
    pub fn handle_servo_messages(
        &self,
        servo: &mut Servo<WebView>,
        events: &mut Vec<EmbedderEvent>,
        status: &mut Status,
    ) -> bool {
        let mut need_present = false;
        servo.get_events().into_iter().for_each(|(w, m)| {
            if w == self.webview_id {
                // TODO Move this to webview
                log::trace!("Verso WebView {w:?} is handling servo message: {m:?}");
                match m {
                    EmbedderMsg::WebViewOpened(w) => {
                        events.push(EmbedderEvent::FocusWebView(w));
                    }
                    EmbedderMsg::AllowNavigationRequest(pipeline_id, _url) => {
                        if w.is_some() {
                            events.push(EmbedderEvent::AllowNavigationResponse(pipeline_id, true));
                        }
                    }
                    EmbedderMsg::WebViewClosed(_w) => {
                        events.push(EmbedderEvent::Quit);
                    }
                    e => {
                        log::warn!(
                            "Verso WebView hasn't supported handling this message yet: {e:?}"
                        )
                    }
                }
            } else {
                log::trace!("Verso Window is handling servo message: {m:?}");
                match m {
                    EmbedderMsg::ReadyToPresent(_w) => {
                        need_present = true;
                    }
                    EmbedderMsg::SetCursor(cursor) => {
                        self.set_cursor_icon(cursor);
                    }
                    EmbedderMsg::Shutdown => {
                        *status = Status::Shutdown;
                    }
                    e => {
                        log::warn!("Verso Window hasn't supported handling this message yet: {e:?}")
                    }
                }
            }
        });

        need_present
    }

    /// Paint offscreen framebuffer to winit window.
    pub fn paint(&self, servo: &mut Servo<WebView>) {
        if let Some(fbo) = servo.offscreen_framebuffer_id() {
            let viewport = self.webview.get_coordinates().get_flipped_viewport();
            let webrender_gl = &self.webrender_gl;

            let target_fbo = self
                .rendering_context
                .context_surface_info()
                .unwrap_or(None)
                .map(|info| info.framebuffer_object)
                .unwrap_or(0);

            webrender_gl.bind_framebuffer(gl::READ_FRAMEBUFFER, fbo);
            webrender_gl.bind_framebuffer(gl::DRAW_FRAMEBUFFER, target_fbo);

            let x = viewport.min.x;
            let y = viewport.min.y;
            let width = viewport.size().width;
            let height = viewport.size().height;
            webrender_gl.blit_framebuffer(
                x,
                y,
                x + width,
                y + height,
                x,
                y,
                x + width,
                y + height,
                gl::COLOR_BUFFER_BIT,
                gl::NEAREST,
            );

            debug_assert_eq!(
                (
                    self.webrender_gl.get_error(),
                    self.webrender_gl.check_frame_buffer_status(gl::FRAMEBUFFER)
                ),
                (gl::NO_ERROR, gl::FRAMEBUFFER_COMPLETE)
            );

            servo.present();
        }
    }

    /// Queues a Winit's [`WindowEvent::RedrawRequested`] event to be emitted that aligns with the windowing system drawing loop.
    pub fn request_redraw(&self) {
        self.window.request_redraw()
    }
    /// Check if window's web view is animating.
    pub fn is_animating(&self) -> bool {
        self.webview.is_animating()
    }

    /// Resize the rendering context.
    pub fn resize(&self, size: Size2D<i32, UnknownUnit>) {
        let _ = self.rendering_context.resize(size);
    }

    /// Set cursor icon of the window.
    pub fn set_cursor_icon(&self, cursor: Cursor) {
        let winit_cursor = match cursor {
            Cursor::Default => CursorIcon::Default,
            Cursor::Pointer => CursorIcon::Pointer,
            Cursor::ContextMenu => CursorIcon::ContextMenu,
            Cursor::Help => CursorIcon::Help,
            Cursor::Progress => CursorIcon::Progress,
            Cursor::Wait => CursorIcon::Wait,
            Cursor::Cell => CursorIcon::Cell,
            Cursor::Crosshair => CursorIcon::Crosshair,
            Cursor::Text => CursorIcon::Text,
            Cursor::VerticalText => CursorIcon::VerticalText,
            Cursor::Alias => CursorIcon::Alias,
            Cursor::Copy => CursorIcon::Copy,
            Cursor::Move => CursorIcon::Move,
            Cursor::NoDrop => CursorIcon::NoDrop,
            Cursor::NotAllowed => CursorIcon::NotAllowed,
            Cursor::Grab => CursorIcon::Grab,
            Cursor::Grabbing => CursorIcon::Grabbing,
            Cursor::EResize => CursorIcon::EResize,
            Cursor::NResize => CursorIcon::NResize,
            Cursor::NeResize => CursorIcon::NeResize,
            Cursor::NwResize => CursorIcon::NwResize,
            Cursor::SResize => CursorIcon::SResize,
            Cursor::SeResize => CursorIcon::SeResize,
            Cursor::SwResize => CursorIcon::SwResize,
            Cursor::WResize => CursorIcon::WResize,
            Cursor::EwResize => CursorIcon::EwResize,
            Cursor::NsResize => CursorIcon::NsResize,
            Cursor::NeswResize => CursorIcon::NeswResize,
            Cursor::NwseResize => CursorIcon::NwseResize,
            Cursor::ColResize => CursorIcon::ColResize,
            Cursor::RowResize => CursorIcon::RowResize,
            Cursor::AllScroll => CursorIcon::AllScroll,
            Cursor::ZoomIn => CursorIcon::ZoomIn,
            Cursor::ZoomOut => CursorIcon::ZoomOut,
            _ => CursorIcon::Default,
        };
        self.window.set_cursor_icon(winit_cursor);
    }
}
