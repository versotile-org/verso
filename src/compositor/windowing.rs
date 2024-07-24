//! Abstract windowing methods. The concrete implementations of these can be found in `platform/`.

use servo::euclid::Scale;
use servo::script_traits::MouseButton;
use servo::servo_geometry::DeviceIndependentPixel;
use servo::style_traits::DevicePixel;
use servo::webrender_traits::RenderingContext;
use webrender::api::units::{DeviceIntPoint, DeviceIntRect, DeviceIntSize, DevicePoint};

#[derive(Clone)]
pub enum MouseWindowEvent {
    Click(MouseButton, DevicePoint),
    MouseDown(MouseButton, DevicePoint),
    MouseUp(MouseButton, DevicePoint),
}

/// Various debug and profiling flags that WebRender supports.
#[derive(Clone)]
pub enum WebRenderDebugOption {
    Profiler,
    TextureCacheDebug,
    RenderTargetDebug,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimationState {
    Idle,
    Animating,
}

// TODO: this trait assumes that the window is responsible
// for creating the GL context, making it current, buffer
// swapping, etc. Really that should all be done by surfman.
pub trait WindowMethods {
    /// Get the coordinates of the native window, the screen and the framebuffer.
    fn get_coordinates(&self) -> EmbedderCoordinates;
    /// Set whether the application is currently animating.
    /// Typically, when animations are active, the window
    /// will want to avoid blocking on UI events, and just
    /// run the event loop at the vsync interval.
    fn set_animation_state(&self, _state: AnimationState);
    /// Get the [`RenderingContext`] of this Window.
    fn rendering_context(&self) -> RenderingContext;
}

#[derive(Clone, Copy, Debug)]
pub struct EmbedderCoordinates {
    /// The pixel density of the display.
    pub hidpi_factor: Scale<f32, DeviceIndependentPixel, DevicePixel>,
    /// Size of the screen.
    pub screen: DeviceIntSize,
    /// Size of the available screen space (screen without toolbars and docks).
    pub screen_avail: DeviceIntSize,
    /// Size of the native window.
    pub window: (DeviceIntSize, DeviceIntPoint),
    /// Size of the GL buffer in the window.
    pub framebuffer: DeviceIntSize,
    /// Coordinates of the document within the framebuffer.
    pub viewport: DeviceIntRect,
}

impl EmbedderCoordinates {
    /// Get the unflipped viewport rectangle for use with the WebRender API.
    pub fn get_viewport(&self) -> DeviceIntRect {
        self.viewport
    }

    /// Flip the given rect.
    /// This should be used when drawing directly to the framebuffer with OpenGL commands.
    pub fn flip_rect(&self, rect: &DeviceIntRect) -> DeviceIntRect {
        let mut result = *rect;
        let min_y = self.framebuffer.height - result.max.y;
        let max_y = self.framebuffer.height - result.min.y;
        result.min.y = min_y;
        result.max.y = max_y;
        result
    }

    /// Get the flipped viewport rectangle.
    /// This should be used when drawing directly to the framebuffer with OpenGL commands.
    pub fn get_flipped_viewport(&self) -> DeviceIntRect {
        self.flip_rect(&self.get_viewport())
    }
}

#[cfg(test)]
mod test {
    use servo::euclid::{Point2D, Scale, Size2D};
    use webrender::api::units::DeviceIntRect;

    use super::EmbedderCoordinates;

    #[test]
    fn test() {
        let pos = Point2D::new(0, 0);
        let viewport = Size2D::new(800, 600);
        let screen = Size2D::new(1080, 720);
        let coordinates = EmbedderCoordinates {
            hidpi_factor: Scale::new(1.),
            screen,
            screen_avail: screen,
            window: (viewport, pos),
            framebuffer: viewport,
            viewport: DeviceIntRect::from_origin_and_size(pos, viewport),
        };

        // Check if viewport conversion is correct.
        let viewport = DeviceIntRect::new(Point2D::new(0, 0), Point2D::new(800, 600));
        assert_eq!(coordinates.get_viewport(), viewport);
        assert_eq!(coordinates.get_flipped_viewport(), viewport);

        // Check rects with different y positions inside the viewport.
        let rect1 = DeviceIntRect::new(Point2D::new(0, 0), Point2D::new(800, 400));
        let rect2 = DeviceIntRect::new(Point2D::new(0, 100), Point2D::new(800, 600));
        let rect3 = DeviceIntRect::new(Point2D::new(0, 200), Point2D::new(800, 500));
        assert_eq!(
            coordinates.flip_rect(&rect1),
            DeviceIntRect::new(Point2D::new(0, 200), Point2D::new(800, 600))
        );
        assert_eq!(
            coordinates.flip_rect(&rect2),
            DeviceIntRect::new(Point2D::new(0, 0), Point2D::new(800, 500))
        );
        assert_eq!(
            coordinates.flip_rect(&rect3),
            DeviceIntRect::new(Point2D::new(0, 100), Point2D::new(800, 400))
        );

        // Check rects with different x positions.
        let rect1 = DeviceIntRect::new(Point2D::new(0, 0), Point2D::new(700, 400));
        let rect2 = DeviceIntRect::new(Point2D::new(100, 100), Point2D::new(800, 600));
        let rect3 = DeviceIntRect::new(Point2D::new(300, 200), Point2D::new(600, 500));
        assert_eq!(
            coordinates.flip_rect(&rect1),
            DeviceIntRect::new(Point2D::new(0, 200), Point2D::new(700, 600))
        );
        assert_eq!(
            coordinates.flip_rect(&rect2),
            DeviceIntRect::new(Point2D::new(100, 0), Point2D::new(800, 500))
        );
        assert_eq!(
            coordinates.flip_rect(&rect3),
            DeviceIntRect::new(Point2D::new(300, 100), Point2D::new(600, 400))
        );
    }
}
