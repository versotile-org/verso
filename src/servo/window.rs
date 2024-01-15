use std::cell::Cell;

use raw_window_handle::HasRawWindowHandle;
use servo::{
  compositing::windowing::{AnimationState, EmbedderCoordinates, WindowMethods},
  config::pref,
  euclid::{Point2D, Scale, Size2D, UnknownUnit},
  webrender_api::units::DeviceIntRect,
  webrender_surfman::WebrenderSurfman,
};
use surfman::{Connection, GLApi, GLVersion, SurfaceType};
// FIXME servo should re-export this.
use servo_media::player::context::{GlApi, GlContext, NativeDisplay};
use winit::window::Window;

/// This is the type for servo embedder. Not for public usage.
pub struct WebView {
  pub webrender_surfman: WebrenderSurfman,
  animation_state: Cell<AnimationState>,
  pub window: Window,
}

impl WebView {
  pub fn new(window: Window) -> Self {
    let connection = Connection::new().expect("Failed to create surfman connection");
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
    }
  }

  pub fn is_animating(&self) -> bool {
    self.animation_state.get() == AnimationState::Animating
  }

  pub fn resize(&self, size: Size2D<i32, UnknownUnit>) {
    let _ = self.webrender_surfman.resize(size);
  }

  pub fn request_redraw(&self) {
    self.window.request_redraw();
  }
}

unsafe impl Send for WebView {}
unsafe impl Sync for WebView {}

impl WindowMethods for WebView {
  fn get_coordinates(&self) -> EmbedderCoordinates {
    let size = self.window.inner_size();
    let pos = Point2D::new(0, 0);
    let viewport = Size2D::new(size.width as i32, size.height as i32);

    let size = self.window.current_monitor().unwrap().size();
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
