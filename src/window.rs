use std::cell::Cell;

use base::id::WebViewId;
use compositing_traits::ConstellationMsg;
use crossbeam_channel::Sender;
use embedder_traits::{Cursor, EmbedderMsg};
use euclid::{Point2D, Size2D};
use glutin::{
    config::{ConfigTemplateBuilder, GlConfig},
    surface::{Surface, WindowSurface},
};
use glutin_winit::DisplayBuilder;
use script_traits::{TouchEventType, WheelDelta, WheelMode};
use servo_url::ServoUrl;
use webrender_api::{
    units::{DeviceIntPoint, DeviceIntRect, DeviceIntSize, DevicePoint, LayoutVector2D},
    ScrollLocation,
};
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, TouchPhase, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::ModifiersState,
    window::{CursorIcon, Window as WinitWindow, WindowId},
};

use crate::{
    compositor::{IOCompositor, MouseWindowEvent},
    keyboard::keyboard_event_from_winit,
    rendering::{gl_config_picker, RenderingContext},
    verso::send_to_constellation,
    webview::WebView,
};

use arboard::Clipboard;

/// A Verso window is a Winit window containing several web views.
pub struct Window {
    /// Access to Winit window
    pub(crate) window: WinitWindow,
    /// GL surface of the window
    pub(crate) surface: Surface<WindowSurface>,
    /// The main panel of this window. A panel is a special web view that focus on controlling states around window.
    /// It could be treated as the control panel or navigation bar of the window depending on usages.
    ///
    /// At the moment, following Web API is supported:
    /// - Close window: `window.close()`
    /// - Navigate to previous page: `window.prompt('PREV')`
    /// - Navigate to next page: `window.prompt('FORWARD')`
    /// - Refresh the page: `window.prompt('REFRESH')`
    /// - Minimize the window: `window.prompt('MINIMIZE')`
    /// - Maximize the window: `window.prompt('MAXIMIZE')`
    /// - Navigate to a specific URL: `window.prompt('NAVIGATE_TO:${url}')`
    pub(crate) panel: Option<WebView>,
    /// The WebView of this window.
    pub(crate) webview: Option<WebView>,
    /// The mouse physical position in the web view.
    mouse_position: Cell<PhysicalPosition<f64>>,
    /// Modifiers state of the keyboard.
    modifiers_state: Cell<ModifiersState>,
}

impl Window {
    /// Create a Verso window from Winit window and return the rendering context.
    pub fn new(evl: &ActiveEventLoop, with_panel: bool) -> (Self, RenderingContext) {
        let window_attributes = WinitWindow::default_attributes()
            .with_transparent(true)
            .with_decorations(false);

        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_transparency(cfg!(macos));

        let (window, gl_config) = DisplayBuilder::new()
            .with_window_attributes(Some(window_attributes))
            .build(evl, template, gl_config_picker)
            .expect("Failed to create window and gl config");

        let window = window.ok_or("Failed to create window").unwrap();

        log::debug!("Picked a config with {} samples", gl_config.num_samples());

        #[cfg(macos)]
        unsafe {
            let rwh = window.window_handle().expect("Failed to get window handle");
            if let RawWindowHandle::AppKit(AppKitWindowHandle { ns_view, .. }) = rwh.as_ref() {
                decorate_window(
                    ns_view.as_ptr() as *mut AnyObject,
                    LogicalPosition::new(8.0, 40.0),
                );
            }
        }
        let (rendering_context, surface) = RenderingContext::create(&window, &gl_config)
            .expect("Failed to create rendering context");
        log::trace!("Created rendering context for window {:?}", window);

        let panel = if with_panel {
            Some(Self::create_control_panel(&window))
        } else {
            None
        };

        (
            Self {
                window,
                surface,
                panel,
                webview: None,
                mouse_position: Cell::new(PhysicalPosition::default()),
                modifiers_state: Cell::new(ModifiersState::default()),
            },
            rendering_context,
        )
    }

    /// Create a Verso window with the rendering context.
    pub fn new_with_compositor(
        evl: &ActiveEventLoop,
        compositor: &mut IOCompositor,
        with_panel: bool,
    ) -> Self {
        let window = evl
            .create_window(WinitWindow::default_attributes())
            // .with_transparent(true)
            // .with_decorations(false)
            .expect("Failed to create window.");

        #[cfg(macos)]
        unsafe {
            let rwh = window.window_handle().expect("Failed to get window handle");
            if let RawWindowHandle::AppKit(AppKitWindowHandle { ns_view, .. }) = rwh.as_ref() {
                decorate_window(
                    ns_view.as_ptr() as *mut AnyObject,
                    LogicalPosition::new(8.0, 40.0),
                );
            }
        }
        let surface = compositor
            .rendering_context
            .create_surface(&window)
            .unwrap();
        let panel = if with_panel {
            Some(Self::create_control_panel(&window))
        } else {
            None
        };
        let mut window = Self {
            window,
            surface,
            panel,
            webview: None,
            mouse_position: Cell::new(PhysicalPosition::default()),
            modifiers_state: Cell::new(ModifiersState::default()),
        };
        compositor.swap_current_window(&mut window);
        window
    }

    /// Create the control panel
    fn create_control_panel(window: &winit::window::Window) -> WebView {
        let size = window.inner_size();
        let size = Size2D::new(size.width as i32, size.height as i32);
        WebView::new(WebViewId::new(), DeviceIntRect::from_size(size))
    }

    /// Get the content area size for the webview to draw on
    pub fn get_content_rect(&self, mut size: DeviceIntRect) -> DeviceIntRect {
        if self.panel.is_some() {
            size.min.y = size.max.y.min(100);
            size.min.x += 10;
            size.max.y -= 10;
            size.max.x -= 10;
        }
        size
    }

    /// Send the constellation message to start Panel UI
    pub fn init_panel_webview(&mut self, constellation_sender: &Sender<ConstellationMsg>) {
        if let Some(panel) = &self.panel {
            let panel_id = panel.webview_id;
            let url = ServoUrl::parse("verso://panel.html").unwrap();
            send_to_constellation(
                constellation_sender,
                ConstellationMsg::NewWebView(url, panel_id),
            );
        }
    }

    /// Handle Winit window event and return a boolean to indicate if the compositor should repaint immediately.
    pub fn handle_winit_window_event(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        compositor: &mut IOCompositor,
        event: &winit::event::WindowEvent,
    ) -> bool {
        match event {
            WindowEvent::RedrawRequested => {
                if let Err(err) = compositor.rendering_context.present(&self.surface) {
                    log::warn!("Failed to present surface: {:?}", err);
                }
            }
            WindowEvent::Focused(focused) => {
                if *focused {
                    compositor.swap_current_window(self);
                }
            }
            WindowEvent::Resized(size) => {
                let size = Size2D::new(size.width, size.height);
                return compositor.resize(size.to_i32(), self);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                compositor.on_scale_factor_event(*scale_factor as f32, self);
            }
            WindowEvent::CursorEntered { .. } => {
                compositor.swap_current_window(self);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor: DevicePoint = DevicePoint::new(position.x as f32, position.y as f32);
                self.mouse_position.set(*position);
                compositor.on_mouse_window_move_event_class(cursor);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button: script_traits::MouseButton = match button {
                    winit::event::MouseButton::Left => script_traits::MouseButton::Left,
                    winit::event::MouseButton::Right => script_traits::MouseButton::Right,
                    winit::event::MouseButton::Middle => script_traits::MouseButton::Middle,
                    _ => {
                        log::trace!(
                            "Verso Window isn't supporting this mouse button yet: {button:?}"
                        );
                        return false;
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
            WindowEvent::PinchGesture { delta, .. } => {
                compositor.on_zoom_window_event(1.0 + *delta as f32, self);
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
            WindowEvent::ModifiersChanged(modifier) => self.modifiers_state.set(modifier.state()),
            WindowEvent::KeyboardInput { event, .. } => {
                let event = keyboard_event_from_winit(event, self.modifiers_state.get());
                log::trace!("Verso is handling {:?}", event);
                let msg = ConstellationMsg::Keyboard(event);
                send_to_constellation(sender, msg);
            }
            e => log::trace!("Verso Window isn't supporting this window event yet: {e:?}"),
        }
        false
    }

    /// Handle servo messages. Return true if it requests a new window
    pub fn handle_servo_message(
        &mut self,
        webview_id: WebViewId,
        message: EmbedderMsg,
        sender: &Sender<ConstellationMsg>,
        clipboard: Option<&mut Clipboard>,
        compositor: &mut IOCompositor,
    ) -> bool {
        // // Handle message in Verso Panel
        if let Some(panel) = &self.panel {
            if panel.webview_id == webview_id {
                return self.handle_servo_messages_with_panel(
                    webview_id, message, sender, clipboard, compositor,
                );
            }
        }
        // Handle message in Verso WebView
        self.handle_servo_messages_with_webview(webview_id, message, sender, clipboard, compositor);
        false
    }

    /// Queues a Winit `WindowEvent::RedrawRequested` event to be emitted that aligns with the windowing system drawing loop.
    pub fn request_redraw(&self) {
        self.window.request_redraw()
    }

    /// Size of the window that's used by webrender.
    pub fn size(&self) -> DeviceIntSize {
        let size = self.window.inner_size();
        Size2D::new(size.width as i32, size.height as i32)
    }

    /// Get Winit window ID of the window.
    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Scale factor of the window. This is also known as HIDPI.
    pub fn scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }

    /// Check if the window has such webview.
    pub fn has_webview(&self, id: WebViewId) -> bool {
        self.panel.as_ref().map_or(false, |w| w.webview_id == id)
            || self.webview.as_ref().map_or(false, |w| w.webview_id == id)
    }

    /// Remove the webview in this window by provided webview ID. If this is the panel, it will
    /// shut down the compositor and then close whole application.
    pub fn remove_webview(
        &mut self,
        id: WebViewId,
        compositor: &mut IOCompositor,
    ) -> (Option<WebView>, bool) {
        if self.panel.as_ref().filter(|w| w.webview_id == id).is_some() {
            if let Some(w) = self.webview.as_ref() {
                send_to_constellation(
                    &compositor.constellation_chan,
                    ConstellationMsg::CloseWebView(w.webview_id),
                )
            }
            (self.panel.take(), false)
        } else if self
            .webview
            .as_ref()
            .filter(|w| w.webview_id == id)
            .is_some()
        {
            (self.webview.take(), self.panel.is_none())
        } else {
            (None, false)
        }
    }

    /// Get the painting order of this window.
    pub fn painting_order(&self) -> Vec<&WebView> {
        let mut order = vec![];
        if let Some(panel) = &self.panel {
            order.push(panel);
        }
        if let Some(webview) = &self.webview {
            order.push(webview);
        }
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
        self.window.set_cursor(winit_cursor);
    }
}

/* window decoration */
#[cfg(macos)]
use objc2::runtime::AnyObject;
#[cfg(macos)]
use raw_window_handle::{AppKitWindowHandle, HasWindowHandle, RawWindowHandle};
#[cfg(macos)]
use winit::dpi::LogicalPosition;

/// Window decoration for macOS.
#[cfg(macos)]
pub unsafe fn decorate_window(view: *mut AnyObject, _position: LogicalPosition<f64>) {
    use objc2::rc::Id;
    use objc2_app_kit::{NSView, NSWindowStyleMask, NSWindowTitleVisibility};

    let ns_view: Id<NSView> = unsafe { Id::retain(view.cast()) }.unwrap();
    let window = ns_view
        .window()
        .expect("view was not installed in a window");
    window.setTitlebarAppearsTransparent(true);
    window.setTitleVisibility(NSWindowTitleVisibility::NSWindowTitleHidden);
    window.setStyleMask(
        NSWindowStyleMask::Titled
            | NSWindowStyleMask::FullSizeContentView
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Resizable
            | NSWindowStyleMask::Miniaturizable,
    );
}
