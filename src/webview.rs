use std::cell::Cell;

use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use servo::{
    compositing::windowing::{
        AnimationState, EmbedderCoordinates, EmbedderEvent, MouseWindowEvent, WindowMethods,
    },
    config::pref,
    euclid::{Point2D, Scale, Size2D, UnknownUnit},
    media::{GlApi, GlContext, NativeDisplay},
    script_traits::{TouchEventType, WheelDelta, WheelMode},
    webrender_api::{
        units::{DeviceIntPoint, DeviceIntRect, DevicePoint, LayoutVector2D},
        ScrollLocation,
    },
    webrender_surfman::WebrenderSurfman,
    Servo,
};
use surfman::{Connection, GLApi, GLVersion, SurfaceType};
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, TouchPhase, WindowEvent},
    window::Window,
};

/// A web view is an area to display web browsing context. It's what user will treat as a "web page".
pub struct WebView {
    /// Access to webrender surfman
    pub webrender_surfman: WebrenderSurfman,
    animation_state: Cell<AnimationState>,
    /// Access to winit winodw
    pub window: Window,
    mouse_position: Cell<PhysicalPosition<f64>>,
}

impl WebView {
    /// Create a web view from winit window.
    pub fn new(window: Window) -> Self {
        let display_handle = window.raw_display_handle();
        let connection = Connection::from_raw_display_handle(display_handle)
            .expect("Failed to create connection");
        let adapter = connection
            .create_adapter()
            .expect("Failed to create surfman adapter");
        let native_widget = connection
            .create_native_widget_from_rwh(window.raw_window_handle())
            .expect("Failed to create surfman native widget");
        let surface_type = SurfaceType::Widget { native_widget };
        let webrender_surfman = WebrenderSurfman::create(&connection, &adapter, surface_type)
            .expect("Failed to create webrender surfman");
        log::trace!("Created webrender surfman for window {:?}", window);

        Self {
            webrender_surfman,
            animation_state: Cell::new(AnimationState::Idle),
            window,
            mouse_position: Cell::new(PhysicalPosition::default()),
        }
    }

    /// Check if web view is animating.
    pub fn is_animating(&self) -> bool {
        self.animation_state.get() == AnimationState::Animating
    }

    /// Resize the web view.
    pub fn resize(&self, size: Size2D<i32, UnknownUnit>) {
        let _ = self.webrender_surfman.resize(size);
    }

    /// Request winit window to emit redraw event.
    pub fn request_redraw(&self) {
        self.window.request_redraw();
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
                servo.recomposite();
                servo.present();
                events.push(EmbedderEvent::Idle);
            }
            WindowEvent::Resized(size) => {
                let size = Size2D::new(size.width, size.height);
                let _ = self.resize(size.to_i32());
                events.push(EmbedderEvent::Resize);
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
                        log::warn!("Yippee hasn't supported this mouse button yet: {button:?}");
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
            e => log::warn!("Yippee hasn't supported this window event yet: {e:?}"),
        }
    }
}

unsafe impl Send for WebView {}
unsafe impl Sync for WebView {}

impl WindowMethods for WebView {
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
            viewport: DeviceIntRect::new(pos, viewport),
        }
    }

    fn set_animation_state(&self, state: AnimationState) {
        self.animation_state.set(state);
    }

    fn get_gl_context(&self) -> GlContext {
        if !pref!(media.glvideo.enabled) {
            return GlContext::Unknown;
        }

        #[allow(unused_variables)]
        let native_context = self.webrender_surfman.native_context();

        #[cfg(target_os = "windows")]
        return GlContext::Egl(native_context.egl_context as usize);

        #[cfg(target_os = "linux")]
        return {
            use surfman::platform::generic::multi::context::NativeContext;
            match native_context {
                NativeContext::Default(NativeContext::Default(native_context)) => {
                    GlContext::Egl(native_context.egl_context as usize)
                }
                NativeContext::Default(NativeContext::Alternate(native_context)) => {
                    GlContext::Egl(native_context.egl_context as usize)
                }
                NativeContext::Alternate(_) => unimplemented!(),
            }
        };

        // @TODO(victor): https://github.com/servo/media/pull/315
        #[cfg(target_os = "macos")]
        #[allow(unreachable_code)]
        return unimplemented!();

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        return unimplemented!();
    }

    fn get_native_display(&self) -> NativeDisplay {
        if !pref!(media.glvideo.enabled) {
            return NativeDisplay::Unknown;
        }

        #[allow(unused_variables)]
        let native_connection = self.webrender_surfman.connection().native_connection();
        #[allow(unused_variables)]
        let native_device = self.webrender_surfman.native_device();

        #[cfg(target_os = "windows")]
        return NativeDisplay::Egl(native_device.egl_display as usize);

        #[cfg(target_os = "linux")]
        return {
            use surfman::platform::generic::multi::connection::NativeConnection;
            match native_connection {
                NativeConnection::Default(NativeConnection::Default(conn)) => {
                    NativeDisplay::Egl(conn.0 as usize)
                }
                NativeConnection::Default(NativeConnection::Alternate(conn)) => {
                    NativeDisplay::X11(conn.x11_display as usize)
                }
                NativeConnection::Alternate(_) => unimplemented!(),
            }
        };

        // @TODO(victor): https://github.com/servo/media/pull/315
        #[cfg(target_os = "macos")]
        #[allow(unreachable_code)]
        return unimplemented!();

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        return unimplemented!();
    }

    fn get_gl_api(&self) -> GlApi {
        let api = self.webrender_surfman.connection().gl_api();
        let attributes = self.webrender_surfman.context_attributes();
        let GLVersion { major, minor } = attributes.version;
        match api {
            GLApi::GL if major >= 3 && minor >= 2 => GlApi::OpenGL3,
            GLApi::GL => GlApi::OpenGL,
            GLApi::GLES if major > 1 => GlApi::Gles2,
            GLApi::GLES => GlApi::Gles1,
        }
    }

    fn webrender_surfman(&self) -> WebrenderSurfman {
        self.webrender_surfman.clone()
    }
}
