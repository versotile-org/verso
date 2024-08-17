use std::ffi::{c_void, CStr};
use std::rc::Rc;
use std::{cell::RefCell, ffi::CString};

use euclid::default::Size2D;
use glutin::context::PossiblyCurrentContext;
use glutin::surface::{Surface as GSurface, WindowSurface};
use glutin::{
    config::{Config, ConfigTemplateBuilder, GlConfig},
    context::{ContextApi, ContextAttributesBuilder, Version},
    display::GetGlDisplay,
    prelude::{GlDisplay, NotCurrentGlContext},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use surfman::{
    Adapter, Connection, Context, ContextAttributeFlags, ContextAttributes, Device, Error, GLApi,
    GLVersion, NativeWidget, Surface, SurfaceAccess, SurfaceInfo, SurfaceType,
};
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

mod gl {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));

    pub use Gles2 as Gl;
}

/// A Verso rendering context, which holds all of the information needed
/// to render Servo's layout, and bridges WebRender and glutin.
#[derive(Clone)]
pub struct RenderingContext(Rc<RenderingContextData>);

struct RContext {
    context: PossiblyCurrentContext,
    surface: GSurface<WindowSurface>,
    gl: gl::Gl,
}

impl RContext {
    /// Create a rendering context instance.
    pub fn create(
        evl: &ActiveEventLoop,
        window: &Window,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_transparency(cfg!(macos));
        let (_, gl_config) = DisplayBuilder::new().build(evl, template, gl_config_picker)?;

        log::debug!("Picked a config with {} samples", gl_config.num_samples());

        // XXX This will panic on Andoird, but we care about Deskyop for now.
        let raw_window_handle = window.window_handle().ok().map(|handle| handle.as_raw());
        // XXX The display could be obtained from any object created by it, so we can
        // query it from the config.
        let gl_display = gl_config.display();
        // The context creation part.
        let context_attributes = ContextAttributesBuilder::new().build(raw_window_handle);
        // Since glutin by default tries to create OpenGL core context, which may not be
        // present we should try gles.
        let fallback_context_attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(None))
            .build(raw_window_handle);
        // There are also some old devices that support neither modern OpenGL nor GLES.
        // To support these we can try and create a 2.1 context.
        let legacy_context_attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(2, 1))))
            .build(raw_window_handle);
        let not_current_gl_context = unsafe {
            gl_display
                .create_context(&gl_config, &context_attributes)
                .unwrap_or_else(|_| {
                    gl_display
                        .create_context(&gl_config, &fallback_context_attributes)
                        .unwrap_or_else(|_| {
                            gl_display
                                .create_context(&gl_config, &legacy_context_attributes)
                                .expect("failed to create context")
                        })
                })
        };

        // Create surface
        let attrs = window
            .build_surface_attributes(Default::default())
            .expect("Failed to build surface attributes");
        let surface = unsafe {
            gl_config
                .display()
                .create_window_surface(&gl_config, &attrs)
                .unwrap()
        };

        // Make it current.
        let context = not_current_gl_context.make_current(&surface).unwrap();

        let gl = gl::Gl::load_with(|symbol| {
            let symbol = CString::new(symbol).unwrap();
            gl_display.get_proc_address(symbol.as_c_str()).cast()
        });

        if let Some(renderer) = get_gl_string(&gl, gl::RENDERER) {
            log::debug!("Running on {}", renderer.to_string_lossy());
        }
        if let Some(version) = get_gl_string(&gl, gl::VERSION) {
            log::debug!("OpenGL Version {}", version.to_string_lossy());
        }

        if let Some(shaders_version) = get_gl_string(&gl, gl::SHADING_LANGUAGE_VERSION) {
            log::debug!("Shaders version on {}", shaders_version.to_string_lossy());
        }

        Ok(Self {
            context,
            surface,
            gl,
        })
    }
}

struct RenderingContextData {
    device: RefCell<Device>,
    context: RefCell<Context>,
}

impl Drop for RenderingContextData {
    fn drop(&mut self) {
        let device = &mut self.device.borrow_mut();
        let context = &mut self.context.borrow_mut();
        let _ = device.destroy_context(context);
    }
}

impl RenderingContext {
    /// Create a rendering context instance.
    pub fn create(
        connection: &Connection,
        adapter: &Adapter,
        surface_type: SurfaceType<NativeWidget>,
    ) -> Result<Self, Error> {
        let mut device = connection.create_device(adapter)?;
        let flags = ContextAttributeFlags::ALPHA
            | ContextAttributeFlags::DEPTH
            | ContextAttributeFlags::STENCIL;
        let version = match connection.gl_api() {
            GLApi::GLES => GLVersion { major: 3, minor: 0 },
            GLApi::GL => GLVersion { major: 3, minor: 2 },
        };
        let context_attributes = ContextAttributes { flags, version };
        let context_descriptor = device.create_context_descriptor(&context_attributes)?;
        let mut context = device.create_context(&context_descriptor, None)?;
        let surface_access = SurfaceAccess::GPUOnly;
        let surface = device.create_surface(&context, surface_access, surface_type)?;
        device
            .bind_surface_to_context(&mut context, surface)
            .map_err(|(err, mut surface)| {
                let _ = device.destroy_surface(&mut context, &mut surface);
                err
            })?;

        device.make_context_current(&context)?;

        let device = RefCell::new(device);
        let context = RefCell::new(context);
        let data = RenderingContextData { device, context };
        Ok(RenderingContext(Rc::new(data)))
    }

    /// Create a surface based on provided surface type.
    pub fn create_surface(
        &self,
        surface_type: SurfaceType<NativeWidget>,
    ) -> Result<Surface, Error> {
        let device = &mut self.0.device.borrow_mut();
        let context = &self.0.context.borrow();
        let surface_access = SurfaceAccess::GPUOnly;
        device.create_surface(context, surface_access, surface_type)
    }

    /// Destroy a surface. A surface must call this before dropping.
    pub fn destroy_surface(&self, mut surface: Surface) -> Result<(), Error> {
        let device = &self.0.device.borrow();
        let context = &mut self.0.context.borrow_mut();
        device.destroy_surface(context, &mut surface)
    }

    /// Make GL context current.
    pub fn make_gl_context_current(&self) -> Result<(), Error> {
        let device = &self.0.device.borrow();
        let context = &self.0.context.borrow();
        device.make_context_current(context)
    }

    /// Resize the rendering context.
    pub fn resize(&self, size: Size2D<i32>) -> Result<(), Error> {
        let device = &mut self.0.device.borrow_mut();
        let context = &mut self.0.context.borrow_mut();
        let mut surface = device.unbind_surface_from_context(context)?.unwrap();
        device.resize_surface(context, &mut surface, size)?;
        device
            .bind_surface_to_context(context, surface)
            .map_err(|(err, mut surface)| {
                let _ = device.destroy_surface(context, &mut surface);
                err
            })
    }

    /// Present the surface of the rendering context.
    pub fn present(&self) -> Result<(), Error> {
        let device = &mut self.0.device.borrow_mut();
        let context = &mut self.0.context.borrow_mut();
        let mut surface = device.unbind_surface_from_context(context)?.unwrap();
        device.present_surface(context, &mut surface)?;
        device
            .bind_surface_to_context(context, surface)
            .map_err(|(err, mut surface)| {
                let _ = device.destroy_surface(context, &mut surface);
                err
            })
    }

    /// Invoke a closure with the surface associated with the current front buffer.
    /// This can be used to create a surfman::SurfaceTexture to blit elsewhere.
    pub fn with_front_buffer<F: FnOnce(&Device, Surface) -> Surface>(&self, f: F) {
        let device = &mut self.0.device.borrow_mut();
        let context = &mut self.0.context.borrow_mut();
        let surface = device
            .unbind_surface_from_context(context)
            .unwrap()
            .unwrap();
        let surface = f(device, surface);
        device.bind_surface_to_context(context, surface).unwrap();
    }

    /// Get the conntection of the rendering context.
    pub fn connection(&self) -> Connection {
        let device = &self.0.device.borrow();
        device.connection()
    }

    /// Get the context surface info of the rendering context.
    pub fn context_surface_info(&self) -> Result<Option<SurfaceInfo>, Error> {
        let device = &self.0.device.borrow();
        let context = &self.0.context.borrow();
        device.context_surface_info(context)
    }
    /// Get the proc address of the rendering context.
    pub fn get_proc_address(&self, name: &str) -> *const c_void {
        let device = &self.0.device.borrow();
        let context = &self.0.context.borrow();
        device.get_proc_address(context, name)
    }
}

/// Find the config with the maximum number of samples, so our triangle will be
/// smooth.
pub fn gl_config_picker(configs: Box<dyn Iterator<Item = Config> + '_>) -> Config {
    configs
        .reduce(|accum, config| {
            let transparency_check = config.supports_transparency().unwrap_or(false)
                & !accum.supports_transparency().unwrap_or(false);

            if transparency_check || config.num_samples() > accum.num_samples() {
                config
            } else {
                accum
            }
        })
        .unwrap()
}

fn get_gl_string(gl: &gl::Gl, variant: gl::types::GLenum) -> Option<&'static CStr> {
    unsafe {
        let s = gl.GetString(variant);
        (!s.is_null()).then(|| CStr::from_ptr(s.cast()))
    }
}
