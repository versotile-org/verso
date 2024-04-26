use std::{cell::Cell, rc::Rc};

use servo::{
    compositing::windowing::{AnimationState, EmbedderCoordinates, WindowMethods},
    euclid::{Point2D, Scale, Size2D},
    rendering_context::RenderingContext,
    webrender_api::units::DeviceIntRect,
};
use winit::window::Window as WinitWindow;

/// A web view is an area to display web browsing context. It's what user will treat as a "web page".
pub struct WebView {
    /// Access to webrender rendering context
    rendering_context: RenderingContext,
    /// Animation state set by Servo to indicate if the webview is still rendering.
    animation_state: Cell<AnimationState>,
    /// Access to winit window
    window: Rc<WinitWindow>,
}

impl WebView {
    /// Create a web view from winit window.
    pub fn new(window: Rc<WinitWindow>, rendering_context: RenderingContext) -> Self {
        Self {
            rendering_context,
            animation_state: Cell::new(AnimationState::Idle),
            window,
        }
    }

    /// Check if web view is animating.
    pub fn is_animating(&self) -> bool {
        self.animation_state.get() == AnimationState::Animating
    }
}

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
