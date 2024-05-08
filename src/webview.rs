use std::{cell::Cell, rc::Rc};

use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use servo::{
    compositing::windowing::{
        AnimationState, EmbedderCoordinates, EmbedderEvent, MouseWindowEvent, WindowMethods,
    },
    euclid::{Point2D, Scale, Size2D, UnknownUnit},
    gl,
    rendering_context::RenderingContext,
    script_traits::{TouchEventType, WheelDelta, WheelMode},
    webrender_api::{
        units::{DeviceIntPoint, DeviceIntRect, DevicePoint, LayoutVector2D},
        ScrollLocation,
    },
    Servo,
};
use surfman::{Connection, GLApi, SurfaceType};
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, TouchPhase, WindowEvent},
    keyboard::ModifiersState,
    window::Window,
};

use crate::keyboard::keyboard_event_from_winit;

/// A web view is an area to display web browsing context. It's what user will treat as a "web page".
pub struct WebView {
    /// Access to webrender surfman
    pub rendering_context: RenderingContext,
    animation_state: Cell<AnimationState>,
    /// Access to winit window
    pub window: Window,
    mouse_position: Cell<PhysicalPosition<f64>>,
    modifiers_state: Cell<ModifiersState>,
    /// Access to webrender gl
    pub webrender_gl: Rc<dyn gl::Gl>,
}

impl WebView {
    /// Create a web view from winit window.
    pub fn new(window: Window) -> Self {
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
            rendering_context,
            animation_state: Cell::new(AnimationState::Idle),
            window,
            mouse_position: Cell::new(PhysicalPosition::default()),
            modifiers_state: Cell::new(ModifiersState::default()),
            webrender_gl,
        }
    }

    /// Check if web view is animating.
    pub fn is_animating(&self) -> bool {
        self.animation_state.get() == AnimationState::Animating
    }

    /// Resize the web view.
    pub fn resize(&self, size: Size2D<i32, UnknownUnit>) {
        let _ = self.rendering_context.resize(size);
    }

    /// Request winit window to emit redraw event.
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Paint offscreen framebuffer to winit window.
    pub fn paint(&self, servo: &mut Servo<WebView>) {
        if let Some(fbo) = servo.offscreen_framebuffer_id() {
            let viewport = self.get_coordinates().get_flipped_viewport();
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
                        log::warn!("Verso hasn't supported this mouse button yet: {button:?}");
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
            // TODO ModifiersChanged and KeyboardInput handling is temporary here. This will be
            // refactored after multiview.
            WindowEvent::ModifiersChanged(modifier) => self.modifiers_state.set(modifier.state()),
            WindowEvent::KeyboardInput { event, .. } => {
                let event = keyboard_event_from_winit(&event, self.modifiers_state.get());
                log::trace!("Verso is handling {:?}", event);
                events.push(EmbedderEvent::Keyboard(event));
            }
            e => log::warn!("Verso hasn't supported this window event yet: {e:?}"),
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
