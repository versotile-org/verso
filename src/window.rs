use std::{cell::Cell, ops::Deref, rc::Rc};

use compositing_traits::ConstellationMsg;
use crossbeam_channel::Sender;
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use servo::{
    compositing::{
        windowing::{AnimationState, EmbedderCoordinates, MouseWindowEvent, WindowMethods},
        IOCompositor,
    },
    embedder_traits::{Cursor, EmbedderMsg},
    euclid::{Point2D, Scale, Size2D},
    gl,
    script_traits::{TouchEventType, WheelDelta, WheelMode},
    style_traits::DevicePixel,
    webrender_api::{
        units::{DeviceIntPoint, DeviceIntRect, DevicePoint, LayoutVector2D},
        ScrollLocation,
    },
    webrender_traits::RenderingContext,
    Servo, TopLevelBrowsingContextId,
};
use surfman::{Connection, GLApi, SurfaceType};
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, TouchPhase, WindowEvent},
    keyboard::ModifiersState,
    window::{CursorIcon, Window as WinitWindow},
};

use crate::{
    keyboard::keyboard_event_from_winit,
    verso::send_to_constellation,
    webview::{Panel, WebView},
};

use arboard::Clipboard;

/// A Verso window is a Winit window containing several web views.
pub struct Window {
    /// Access to Winit window with webrender context.
    pub(crate) gl_window: Rc<GLWindow>,
    /// The main control panel of this window.
    pub(crate) panel: Panel,
    /// The WebView of this window.
    pub(crate) webview: Option<WebView>,
    /// Access to webrender GL
    pub(crate) webrender_gl: Rc<dyn gl::Gl>,
    /// The mouse physical position in the web view.
    mouse_position: Cell<PhysicalPosition<f64>>,
    /// Modifiers state of the keyboard.
    modifiers_state: Cell<ModifiersState>,
}

impl Window {
    /// Create a Verso window from Winit window.
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

        Self {
            gl_window: Rc::new(GLWindow::new(window, rendering_context)),
            panel: Panel::new(),
            webview: None,
            webrender_gl,
            mouse_position: Cell::new(PhysicalPosition::default()),
            modifiers_state: Cell::new(ModifiersState::default()),
        }
    }

    /// Return the reference counted `GLWindow`.
    pub fn gl_window(&self) -> Rc<GLWindow> {
        return self.gl_window.clone();
    }

    /// Handle Winit window event.
    pub fn handle_winit_window_event(
        &self,
        sender: &Sender<ConstellationMsg>,
        compositor: &mut IOCompositor<GLWindow>,
        event: &winit::event::WindowEvent,
    ) {
        match event {
            WindowEvent::RedrawRequested => {
                compositor.present();
            }
            WindowEvent::Resized(size) => {
                let size = Size2D::new(size.width, size.height);
                let _ = self.resize(size.to_i32(), compositor);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor: DevicePoint = DevicePoint::new(position.x as f32, position.y as f32);
                self.mouse_position.set(*position);
                compositor.on_mouse_window_move_event_class(cursor);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button: servo::script_traits::MouseButton = match button {
                    winit::event::MouseButton::Left => servo::script_traits::MouseButton::Left,
                    winit::event::MouseButton::Right => servo::script_traits::MouseButton::Right,
                    winit::event::MouseButton::Middle => servo::script_traits::MouseButton::Middle,
                    _ => {
                        log::warn!(
                            "Verso Window isn't supporting this mouse button yet: {button:?}"
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
                compositor.on_mouse_window_event_class(event);

                // Winit didn't send click event, so we send it after mouse up
                if *state == ElementState::Released {
                    let event: MouseWindowEvent = MouseWindowEvent::Click(button, position);
                    compositor.on_mouse_window_event_class(event);
                }
            }
            WindowEvent::TouchpadMagnify { delta, .. } => {
                compositor.on_zoom_window_event(1.0 + *delta as f32);
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
                compositor.on_wheel_event(
                    WheelDelta { x, y, z: 0.0, mode },
                    DevicePoint::new(
                        self.mouse_position.get().x as f32,
                        self.mouse_position.get().y as f32,
                    ),
                );

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

                compositor.on_scroll_event(
                    ScrollLocation::Delta(LayoutVector2D::new(x as f32, y as f32)),
                    DeviceIntPoint::new(
                        self.mouse_position.get().x as i32,
                        self.mouse_position.get().y as i32,
                    ),
                    phase,
                );
            }
            WindowEvent::CloseRequested => {
                compositor.maybe_start_shutting_down();
            }
            WindowEvent::ModifiersChanged(modifier) => self.modifiers_state.set(modifier.state()),
            WindowEvent::KeyboardInput { event, .. } => {
                let event = keyboard_event_from_winit(&event, self.modifiers_state.get());
                log::trace!("Verso is handling {:?}", event);
                let msg = ConstellationMsg::Keyboard(event);
                send_to_constellation(sender, msg);
            }
            e => log::warn!("Verso Window isn't supporting this window event yet: {e:?}"),
        }
    }

    /// Handle servo messages.
    pub fn handle_servo_message(
        &mut self,
        webview_id: Option<TopLevelBrowsingContextId>,
        message: EmbedderMsg,
        sender: &Sender<ConstellationMsg>,
        compositor: &mut IOCompositor<GLWindow>,
        clipboard: &mut Clipboard,
    ) {
        match webview_id {
            // // Handle message in Verso Panel
            Some(p) if p == self.panel.id() => {
                self.handle_servo_messages_with_panel(p, message, sender, compositor, clipboard);
            }
            // Handle message in Verso WebView
            Some(w) => {
                self.handle_servo_messages_with_webview(w, message, sender, compositor, clipboard);
            }
            // Handle message in Verso Window
            None => {
                log::trace!("Verso Window is handling Embedder message: {message:?}");
                match message {
                    EmbedderMsg::ReadyToPresent(_w) => {
                        self.window.request_redraw();
                    }
                    EmbedderMsg::SetCursor(cursor) => {
                        self.set_cursor_icon(cursor);
                    }
                    EmbedderMsg::Shutdown => {}
                    e => {
                        log::warn!("Verso Window isn't supporting handling this message yet: {e:?}")
                    }
                }
            }
        }
    }

    /// Paint offscreen framebuffer to Winit window.
    pub fn paint(&self, servo: &mut Servo<GLWindow>) {
        if let Some(fbo) = servo.offscreen_framebuffer_id() {
            let viewport = self.gl_window.get_coordinates().get_flipped_viewport();
            let webrender_gl = &self.webrender_gl;

            let target_fbo = self
                .gl_window
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

    /// Queues a Winit `WindowEvent::RedrawRequested` event to be emitted that aligns with the windowing system drawing loop.
    pub fn request_redraw(&self) {
        self.window.request_redraw()
    }

    /// Check if WebView (`GLWindow`) is animating.
    pub fn is_animating(&self) -> bool {
        self.gl_window.is_animating()
    }

    /// Resize the rendering context and all web views.
    pub fn resize(&self, size: Size2D<i32, DevicePixel>, compositor: &mut IOCompositor<GLWindow>) {
        let _ = self.gl_window.rendering_context.resize(size.to_untyped());
        let need_resize = compositor.on_resize_window_event();

        let rect = DeviceIntRect::from_size(size).to_f32();
        compositor.move_resize_webview(self.panel.id(), rect);

        if let Some(w) = &self.webview {
            let mut rect = DeviceIntRect::from_size(size).to_f32();
            rect.min.y = rect.max.y.min(76.);
            compositor.move_resize_webview(w.id(), rect);
        }

        if need_resize {
            compositor.repaint_synchronously();
            compositor.present();
        }
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

/// A Winit window with webrender rendering context.
pub struct GLWindow {
    /// Access to webrender rendering context
    rendering_context: RenderingContext,
    /// Animation state set by Servo to indicate if the webview is still rendering.
    animation_state: Cell<AnimationState>,
    /// Access to Winit window
    pub(crate) window: WinitWindow,
}

impl GLWindow {
    /// Create a web view from Winit window.
    pub fn new(window: WinitWindow, rendering_context: RenderingContext) -> Self {
        Self {
            rendering_context,
            animation_state: Cell::new(AnimationState::Idle),
            window,
        }
    }

    /// Check if WebView (`GLWindow`) is animating.
    pub fn is_animating(&self) -> bool {
        self.animation_state.get() == AnimationState::Animating
    }
}

impl WindowMethods for GLWindow {
    fn get_coordinates(&self) -> EmbedderCoordinates {
        let size = self.window.inner_size();
        let pos = Point2D::new(0, 0);
        let viewport = Size2D::new(size.width as i32, size.height as i32);

        let size = self.window.available_monitors().nth(0).unwrap().size();
        let screen = Size2D::new(size.width as i32, size.height as i32);
        EmbedderCoordinates {
            hidpi_factor: Scale::new(self.window.scale_factor() as f32),
            screen,
            screen_avail: screen,
            window: (viewport, pos),
            framebuffer: viewport,
            viewport: DeviceIntRect::from_origin_and_size(pos, viewport),
        }
    }

    fn set_animation_state(&self, state: AnimationState) {
        self.animation_state.set(state);
    }

    fn rendering_context(&self) -> RenderingContext {
        self.rendering_context.clone()
    }
}

impl Deref for Window {
    type Target = GLWindow;

    fn deref(&self) -> &Self::Target {
        &self.gl_window
    }
}
