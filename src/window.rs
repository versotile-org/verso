use std::cell::Cell;

use compositing_traits::ConstellationMsg;
use crossbeam_channel::Sender;
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use servo::{
    base::id::{PipelineId, WebViewId},
    embedder_traits::{Cursor, EmbedderMsg},
    euclid::{Point2D, Size2D},
    script_traits::{TouchEventType, WheelDelta, WheelMode},
    style_traits::DevicePixel,
    webrender_api::{
        units::{DeviceIntPoint, DeviceIntRect, DevicePoint, LayoutVector2D},
        ScrollLocation,
    },
    webrender_traits::RenderingContext,
    TopLevelBrowsingContextId,
};
use surfman::{Connection, SurfaceType};
use webrender::api::units::DeviceIntSize;
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, TouchPhase, WindowEvent},
    keyboard::ModifiersState,
    window::{CursorIcon, Window as WinitWindow},
};

use crate::{
    compositor::{IOCompositor, MouseWindowEvent},
    keyboard::keyboard_event_from_winit,
    verso::send_to_constellation,
    webview::WebView,
};

use arboard::Clipboard;

/// A Verso window is a Winit window containing several web views.
pub struct Window {
    /// Access to Winit window
    pub(crate) window: WinitWindow,
    /// The main control panel of this window.
    pub(crate) panel: WebView,
    /// The WebView of this window.
    pub(crate) webview: Option<WebView>,
    /// The mouse physical position in the web view.
    mouse_position: Cell<PhysicalPosition<f64>>,
    /// Modifiers state of the keyboard.
    modifiers_state: Cell<ModifiersState>,
}

impl Window {
    /// Create a Verso window from Winit window and return the rendering context.
    pub fn new_with_context(window: WinitWindow) -> (Self, RenderingContext) {
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

        let size = window.inner_size();
        let size = Size2D::new(size.width as i32, size.height as i32);
        (
            Self {
                window,
                panel: WebView::new_panel(DeviceIntRect::from_size(size)),
                webview: None,
                mouse_position: Cell::new(PhysicalPosition::default()),
                modifiers_state: Cell::new(ModifiersState::default()),
            },
            rendering_context,
        )
    }

    /// Handle Winit window event.
    pub fn handle_winit_window_event(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        compositor: &mut IOCompositor,
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
        clipboard: &mut Clipboard,
    ) {
        match webview_id {
            // // Handle message in Verso Panel
            Some(p) if p == self.panel.webview_id => {
                self.handle_servo_messages_with_panel(p, message, sender, clipboard);
            }
            // Handle message in Verso WebView
            Some(w) => {
                self.handle_servo_messages_with_webview(w, message, sender, clipboard);
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

    /// Queues a Winit `WindowEvent::RedrawRequested` event to be emitted that aligns with the windowing system drawing loop.
    pub fn request_redraw(&self) {
        self.window.request_redraw()
    }

    /// Resize the rendering context and all web views.
    pub fn resize(&mut self, size: Size2D<i32, DevicePixel>, compositor: &mut IOCompositor) {
        let need_resize = compositor.on_resize_window_event(size);

        let rect = DeviceIntRect::from_size(size);
        compositor.on_resize_webview_event(self.panel.webview_id, rect);

        if let Some(w) = &self.webview {
            let mut rect = DeviceIntRect::from_size(size);
            rect.min.y = rect.max.y.min(76);
            compositor.on_resize_webview_event(w.webview_id, rect);
        }

        if need_resize {
            compositor.repaint_synchronously(self);
            compositor.present();
        }
    }

    /// Size of the window that's used by webrender.
    pub fn size(&self) -> DeviceIntSize {
        let size = self.window.inner_size();
        Size2D::new(size.width as i32, size.height as i32)
    }

    /// Scale factor of the window. This is also known as HIDPI.
    pub fn scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }

    /// Get the mutable reference of the webview in this window from provided webview ID.
    pub fn get_webview(&mut self, id: WebViewId) -> Option<&mut WebView> {
        if self.panel.webview_id == id {
            Some(&mut self.panel)
        } else {
            self.webview.as_mut().filter(|w| w.webview_id == id)
        }
    }

    /// Set the webview to this window. It won't be updated if the exisitng webview and pipeline ID
    /// are the same. This will also set the painting order of the compositor and tell
    /// constellation to focus the webview.
    pub fn set_webview(
        &mut self,
        webview_id: WebViewId,
        pipline_id: PipelineId,
        compositor: &mut IOCompositor,
    ) {
        if self.panel.webview_id == webview_id {
            if self.panel.pipeline_id != Some(pipline_id) {
                self.panel.pipeline_id = Some(pipline_id);
            }
        } else if let Some(webview) = &mut self.webview {
            if webview.webview_id == webview_id && webview.pipeline_id != Some(pipline_id) {
                webview.pipeline_id = Some(pipline_id);
            }
        } else {
            let size = self.size();
            let mut rect = DeviceIntRect::from_size(size);
            rect.min.y = rect.max.y.min(76);
            self.webview = Some(WebView::new(webview_id, rect));
        }

        compositor.set_painting_order(self.paiting_order());
        self.resize(self.size(), compositor);

        send_to_constellation(
            &compositor.constellation_chan,
            ConstellationMsg::FocusWebView(webview_id),
        );
    }

    /// Remove the webview in this window by provided webview ID. If this is the panel, it will
    /// shut down the compositor and then close whole application.
    pub fn remove_webview(
        &mut self,
        id: WebViewId,
        compositor: &mut IOCompositor,
    ) -> Option<WebView> {
        if id == self.panel.webview_id {
            compositor.maybe_start_shutting_down();
            None
        } else if self
            .webview
            .as_ref()
            .filter(|w| w.webview_id == id)
            .is_some()
        {
            self.webview.take()
        } else {
            None
        }
    }

    /// Get the painting order of this window.
    pub fn paiting_order(&self) -> Vec<WebView> {
        let mut order = vec![];
        if let Some(webview) = &self.webview {
            order.push(webview.clone());
        }
        order.push(self.panel.clone());
        order
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
