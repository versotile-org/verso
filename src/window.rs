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
#[cfg(any(linux, target_os = "windows"))]
use winit::window::ResizeDirection;
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
    webview::{Panel, WebView},
};

use arboard::Clipboard;

/// A Verso window is a Winit window containing several web views.
pub struct Window {
    /// Access to Winit window
    pub(crate) window: WinitWindow,
    /// GL surface of the window
    pub(crate) surface: Surface<WindowSurface>,
    /// The main panel of this window.
    pub(crate) panel: Option<Panel>,
    /// The WebView of this window.
    pub(crate) webview: Option<WebView>,
    /// The mouse physical position in the web view.
    mouse_position: Cell<Option<PhysicalPosition<f64>>>,
    /// Modifiers state of the keyboard.
    modifiers_state: Cell<ModifiersState>,
}

impl Window {
    /// Create a Verso window from Winit window and return the rendering context.
    pub fn new(evl: &ActiveEventLoop) -> (Self, RenderingContext) {
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

        (
            Self {
                window,
                surface,
                panel: None,
                webview: None,
                mouse_position: Default::default(),
                modifiers_state: Cell::new(ModifiersState::default()),
            },
            rendering_context,
        )
    }

    /// Create a Verso window with the rendering context.
    pub fn new_with_compositor(evl: &ActiveEventLoop, compositor: &mut IOCompositor) -> Self {
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

        let mut window = Self {
            window,
            surface,
            panel: None,
            webview: None,
            mouse_position: Default::default(),
            modifiers_state: Cell::new(ModifiersState::default()),
        };
        compositor.swap_current_window(&mut window);
        window
    }

    /// Get the content area size for the webview to draw on
    pub fn get_content_size(&self, mut size: DeviceIntRect) -> DeviceIntRect {
        if self.panel.is_some() {
            size.min.y = size.max.y.min(100);
            size.min.x += 10;
            size.max.y -= 10;
            size.max.x -= 10;
        }
        size
    }

    /// Send the constellation message to start Panel UI
    pub fn create_panel(
        &mut self,
        constellation_sender: &Sender<ConstellationMsg>,
        initial_url: Option<url::Url>,
    ) {
        let size = self.window.inner_size();
        let size = Size2D::new(size.width as i32, size.height as i32);
        let panel_id = WebViewId::new();
        self.panel = Some(Panel {
            webview: WebView::new(panel_id, DeviceIntRect::from_size(size)),
            initial_url: if let Some(initial_url) = initial_url {
                servo_url::ServoUrl::from_url(initial_url)
            } else {
                ServoUrl::parse("https://example.com").unwrap()
            },
        });

        let url = ServoUrl::parse("verso://panel.html").unwrap();
        send_to_constellation(
            constellation_sender,
            ConstellationMsg::NewWebView(url, panel_id),
        );
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
            WindowEvent::CursorLeft { .. } => {
                self.mouse_position.set(None);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor: DevicePoint = DevicePoint::new(position.x as f32, position.y as f32);
                self.mouse_position.set(Some(*position));
                compositor.on_mouse_window_move_event_class(cursor);

                // handle Windows and Linux non-decoration window resize cursor
                #[cfg(any(linux, target_os = "windows"))]
                {
                    let direction = self.get_drag_resize_direction();
                    self.set_drag_resize_cursor(direction);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let position = match self.mouse_position.get() {
                    Some(position) => Point2D::new(position.x as f32, position.y as f32),
                    None => {
                        log::trace!("Mouse position is None, skipping MouseInput event.");
                        return false;
                    }
                };

                // handle Windows and Linux non-decoration window resize
                #[cfg(any(linux, target_os = "windows"))]
                {
                    if *state == ElementState::Pressed && *button == winit::event::MouseButton::Left
                    {
                        self.drag_resize_window();
                    }
                }

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
                let position = match self.mouse_position.get() {
                    Some(position) => position,
                    None => {
                        log::trace!("Mouse position is None, skipping MouseWheel event.");
                        return false;
                    }
                };

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
                    DevicePoint::new(position.x as f32, position.y as f32),
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
                    DeviceIntPoint::new(position.x as i32, position.y as i32),
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
            if panel.webview.webview_id == webview_id {
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
        self.panel
            .as_ref()
            .map_or(false, |w| w.webview.webview_id == id)
            || self.webview.as_ref().map_or(false, |w| w.webview_id == id)
    }

    /// Remove the webview in this window by provided webview ID. If this is the panel, it will
    /// shut down the compositor and then close whole application.
    pub fn remove_webview(
        &mut self,
        id: WebViewId,
        compositor: &mut IOCompositor,
    ) -> (Option<WebView>, bool) {
        if self
            .panel
            .as_ref()
            .filter(|w| w.webview.webview_id == id)
            .is_some()
        {
            if let Some(w) = self.webview.as_ref() {
                send_to_constellation(
                    &compositor.constellation_chan,
                    ConstellationMsg::CloseWebView(w.webview_id),
                )
            }
            (self.panel.take().map(|panel| panel.webview), false)
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
            order.push(&panel.webview);
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

// Non-decorated window resizing for Windows and Linux.
#[cfg(any(linux, target_os = "windows"))]
impl Window {
    /// Drag resize the window.
    fn drag_resize_window(&self) {
        if let Some(direction) = self.get_drag_resize_direction() {
            if let Err(err) = self.window.drag_resize_window(direction) {
                log::error!("Failed to drag-resize window: {:?}", err);
            }
        }
    }

    /// Get drag-resize direction.
    fn get_drag_resize_direction(&self) -> Option<ResizeDirection> {
        let mouse_position = match self.mouse_position.get() {
            Some(position) => position,
            None => {
                log::trace!("Mouse position is None, skipping drag-resize window");
                return None;
            }
        };

        let window_size = self.window.outer_size();
        let border_size = 5.0 * self.window.scale_factor();

        let x_direction = if mouse_position.x < border_size {
            ResizeDirection::West
        } else if mouse_position.x > (window_size.width as f64 - border_size) {
            ResizeDirection::East
        } else {
            // Use arbitrary direction instead of None for simplicity.
            ResizeDirection::SouthEast
        };

        let y_direction = if mouse_position.y < border_size {
            ResizeDirection::North
        } else if mouse_position.y > (window_size.height as f64 - border_size) {
            ResizeDirection::South
        } else {
            // Use arbitrary direction instead of None for simplicity.
            ResizeDirection::SouthEast
        };

        let direction = match (x_direction, y_direction) {
            (ResizeDirection::East, ResizeDirection::North) => ResizeDirection::NorthEast,
            (ResizeDirection::East, ResizeDirection::South) => ResizeDirection::SouthEast,
            (ResizeDirection::West, ResizeDirection::North) => ResizeDirection::NorthWest,
            (ResizeDirection::West, ResizeDirection::South) => ResizeDirection::SouthWest,
            (ResizeDirection::East, _) => ResizeDirection::East,
            (ResizeDirection::West, _) => ResizeDirection::West,
            (_, ResizeDirection::South) => ResizeDirection::South,
            (_, ResizeDirection::North) => ResizeDirection::North,
            _ => return None,
        };

        Some(direction)
    }

    /// Set drag-resize cursor when mouse is hover on the border of the window.
    fn set_drag_resize_cursor(&self, direction: Option<ResizeDirection>) {
        let cursor = match direction {
            Some(direction) => match direction {
                ResizeDirection::East => CursorIcon::EResize,
                ResizeDirection::West => CursorIcon::WResize,
                ResizeDirection::South => CursorIcon::SResize,
                ResizeDirection::North => CursorIcon::NResize,
                ResizeDirection::NorthEast => CursorIcon::NeResize,
                ResizeDirection::NorthWest => CursorIcon::NwResize,
                ResizeDirection::SouthEast => CursorIcon::SeResize,
                ResizeDirection::SouthWest => CursorIcon::SwResize,
            },
            None => CursorIcon::Default,
        };

        self.window.set_cursor(cursor);
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
