use std::{
    borrow::Cow,
    sync::{atomic::Ordering, Arc},
};

use compositing_traits::{
    CompositorMsg, CompositorProxy, CompositorReceiver, ConstellationMsg, ForwardedToCompositorMsg,
};
use crossbeam_channel::{unbounded, Sender};
use servo::{
    base::id::{PipelineNamespace, PipelineNamespaceId},
    bluetooth::BluetoothThreadFactory,
    bluetooth_traits::BluetoothRequest,
    canvas::{
        canvas_paint_thread::{self, CanvasPaintThread},
        WebGLComm,
    },
    compositing::{
        windowing::WindowMethods, CompositeTarget, IOCompositor, InitialCompositorState,
    },
    config::{opts, pref},
    constellation::{Constellation, InitialConstellationState},
    devtools,
    embedder_traits::{EmbedderMsg, EmbedderProxy, EmbedderReceiver, EventLoopWaker},
    euclid::Scale,
    fonts::FontCacheThread,
    gl,
    ipc_channel::ipc::{self, IpcSender},
    layout_thread_2020,
    media::{GlApi, GlContext, NativeDisplay, WindowGLContext},
    net::resource_thread,
    profile,
    script::{self, JSEngineSetup},
    script_traits::WindowSizeData,
    style, webgpu,
    webrender_traits::*,
    TopLevelBrowsingContextId,
};
use surfman::GLApi;
use webrender::{api::*, ShaderPrecacheFlags};
use winit::{event_loop::EventLoopProxy, window::Window as WinitWindow};

use crate::{
    config::Config,
    window::{GLWindow, Window},
};

/// Main entry point of Verso browser.
pub struct Verso {
    window: Window,
    webview_id: TopLevelBrowsingContextId,
    compositor: IOCompositor<GLWindow>,
    constellation_sender: Sender<ConstellationMsg>,
    embedder_receiver: EmbedderReceiver,
    messages_for_embedder: Vec<(Option<TopLevelBrowsingContextId>, EmbedderMsg)>,
    profiler_enabled: bool,
    /// For single-process Servo instances, this field controls the initialization
    /// and deinitialization of the JS Engine. Multiprocess Servo instances have their
    /// own instance that exists in the content process instead.
    _js_engine_setup: Option<JSEngineSetup>,
}

impl Verso {
    /// Create a Verso instance from Winit's window and event loop proxy.
    ///
    /// // TODO list the flag to toggle them and ways to disable by default
    /// Following threads will be created while initializing Veros:
    /// - Time Profiler
    /// - Memory Profiler
    /// - DevTools
    /// - Webrender threads
    /// - WebGL
    /// - WebXR
    /// - Bluetooth
    /// - Resource threads
    /// - Font cache
    /// - Canvas
    /// - Constellation
    pub fn new(window: WinitWindow, proxy: EventLoopProxy<()>, config: Config) -> Self {
        config.init();
        let window = Window::new(window);
        let event_loop_waker = Box::new(Waker(proxy));
        let opts = opts::get();

        style::context::DEFAULT_DISABLE_STYLE_SHARING_CACHE
            .store(opts.debug.disable_share_style_cache, Ordering::Relaxed);
        style::context::DEFAULT_DUMP_STYLE_STATISTICS
            .store(opts.debug.dump_style_statistics, Ordering::Relaxed);
        style::traversal::IS_SERVO_NONINCREMENTAL_LAYOUT
            .store(opts.nonincremental_layout, Ordering::Relaxed);

        // TODO init media platform

        let user_agent: Cow<'static, str> = default_user_agent_string().into();

        // Initialize surfman & get GL bindings
        let rendering_context = window.rendering_context();
        let webrender_gl = match rendering_context.connection().gl_api() {
            GLApi::GL => unsafe { gl::GlFns::load_with(|s| rendering_context.get_proc_address(s)) },
            GLApi::GLES => unsafe {
                gl::GlesFns::load_with(|s| rendering_context.get_proc_address(s))
            },
        };
        // Make sure the gl context is made current.
        rendering_context.make_gl_context_current().unwrap();
        debug_assert_eq!(webrender_gl.get_error(), gl::NO_ERROR,);
        // Bind the webrender framebuffer
        let framebuffer_object = rendering_context
            .context_surface_info()
            .unwrap_or(None)
            .map(|info| info.framebuffer_object)
            .unwrap_or(0);
        webrender_gl.bind_framebuffer(gl::FRAMEBUFFER, framebuffer_object);

        // Reserving a namespace to create TopLevelBrowsingContextId.
        PipelineNamespace::install(PipelineNamespaceId(0));
        let top_level_browsing_context_id = TopLevelBrowsingContextId::new();

        // Create compositor and embedder channels
        let (compositor_sender, compositor_receiver) = {
            let (sender, receiver) = unbounded();
            (
                CompositorProxy {
                    sender,
                    event_loop_waker: event_loop_waker.clone(),
                },
                CompositorReceiver { receiver },
            )
        };
        let (embedder_sender, embedder_receiver) = {
            let (sender, receiver) = unbounded();
            (
                EmbedderProxy {
                    sender,
                    event_loop_waker: event_loop_waker.clone(),
                },
                EmbedderReceiver { receiver },
            )
        };

        // Create profiler threads
        let time_profiler_sender = profile::time::Profiler::create(
            &opts.time_profiling,
            opts.time_profiler_trace_path.clone(),
        );
        let mem_profiler_sender = profile::mem::Profiler::create(opts.mem_profiler_period);

        // Create dev tools thread
        let devtools_sender = if opts.devtools_server_enabled {
            Some(devtools::start_server(
                opts.devtools_port,
                embedder_sender.clone(),
            ))
        } else {
            None
        };

        // Create Webrender threads
        let coordinates = window.get_coordinates();
        let device_pixel_ratio = coordinates.hidpi_factor.get();
        let viewport_size = coordinates.viewport.size().to_f32() / device_pixel_ratio;
        let (mut webrender, webrender_api_sender) = {
            let mut debug_flags = webrender::DebugFlags::empty();
            debug_flags.set(
                webrender::DebugFlags::PROFILER_DBG,
                opts.debug.webrender_stats,
            );

            let render_notifier = Box::new(RenderNotifier::new(compositor_sender.clone()));
            let clear_color = ColorF::new(1., 1., 1., 0.);
            webrender::create_webrender_instance(
                webrender_gl.clone(),
                render_notifier,
                webrender::WebRenderOptions {
                    // We force the use of optimized shaders here because rendering is broken
                    // on Android emulators with unoptimized shaders. This is due to a known
                    // issue in the emulator's OpenGL emulation layer.
                    // See: https://github.com/servo/servo/issues/31726
                    use_optimized_shaders: true,
                    resource_override_path: opts.shaders_dir.clone(),
                    enable_aa: !opts.debug.disable_text_antialiasing,
                    debug_flags,
                    precache_flags: if opts.debug.precache_shaders {
                        ShaderPrecacheFlags::FULL_COMPILE
                    } else {
                        ShaderPrecacheFlags::empty()
                    },
                    enable_subpixel_aa: pref!(gfx.subpixel_text_antialiasing.enabled)
                        && !opts.debug.disable_subpixel_text_antialiasing,
                    allow_texture_swizzling: pref!(gfx.texture_swizzling.enabled),
                    clear_color,
                    ..Default::default()
                },
                None,
            )
            .expect("Unable to initialize webrender!")
        };
        let webrender_api = webrender_api_sender.create_api();
        let webrender_document = webrender_api.add_document(coordinates.get_viewport().size());

        let js_engine_setup = if !opts.multiprocess {
            Some(script::init())
        } else {
            None
        };

        // Create the webgl thread
        let gl_type = match webrender_gl.get_type() {
            gl::GlType::Gl => sparkle::gl::GlType::Gl,
            gl::GlType::Gles => sparkle::gl::GlType::Gles,
        };
        let (external_image_handlers, external_images) = WebrenderExternalImageHandlers::new();
        let mut external_image_handlers = Box::new(external_image_handlers);
        let WebGLComm {
            webgl_threads,
            webxr_layer_grand_manager,
            image_handler,
        } = WebGLComm::new(
            rendering_context.clone(),
            webrender_api.create_sender(),
            webrender_document,
            external_images.clone(),
            gl_type,
        );
        // Set webrender external image handler for WebGL textures
        external_image_handlers.set_handler(image_handler, WebrenderImageHandlerType::WebGL);

        // Create WebXR dummy
        let webxr_registry =
            webxr::MainThreadRegistry::new(event_loop_waker, webxr_layer_grand_manager)
                .expect("Failed to create WebXR device registry");
        // if pref!(dom.webxr.enabled) {
        // TODO if pref!(dom.webxr.test) {
        //     webxr_main_thread.register_mock(webxr::headless::HeadlessMockDiscovery::new());
        // }
        // else if let Some(xr_discovery) = self.xr_discovery.take() {
        //     webxr_main_thread.register(xr_discovery);
        // }
        // }

        // Set webrender external image handler for WebGPU textures
        let wgpu_image_handler = webgpu::WGPUExternalImages::default();
        let wgpu_image_map = wgpu_image_handler.images.clone();
        external_image_handlers.set_handler(
            Box::new(wgpu_image_handler),
            WebrenderImageHandlerType::WebGPU,
        );

        // TODO enable gl media player
        let glplayer_context = WindowGLContext {
            gl_context: GlContext::Unknown,
            gl_api: GlApi::None,
            native_display: NativeDisplay::Unknown,
            glplayer_chan: None,
        };

        webrender.set_external_image_handler(external_image_handlers);

        // The division by 1 represents the page's default zoom of 100%,
        // and gives us the appropriate CSSPixel type for the viewport.
        let window_size = WindowSizeData {
            initial_viewport: viewport_size / Scale::new(1.0),
            device_pixel_ratio: Scale::new(device_pixel_ratio),
        };

        // Create bluetooth thread
        let bluetooth_thread: IpcSender<BluetoothRequest> =
            BluetoothThreadFactory::new(embedder_sender.clone());

        let (public_resource_threads, private_resource_threads) =
            resource_thread::new_resource_threads(
                user_agent.clone(),
                devtools_sender.clone(),
                time_profiler_sender.clone(),
                mem_profiler_sender.clone(),
                embedder_sender.clone(),
                opts.config_dir.clone(),
                opts.certificate_path.clone(),
                opts.ignore_certificate_errors,
            );

        // Create font cache thread
        let font_cache_thread = FontCacheThread::new(Box::new(WebRenderFontApiCompositorProxy(
            compositor_sender.clone(),
        )));

        // Create canvas thread
        let (canvas_create_sender, canvas_ipc_sender) = CanvasPaintThread::start(
            Box::new(CanvasWebrenderApi(compositor_sender.clone())),
            font_cache_thread.clone(),
            public_resource_threads.clone(),
        );

        let layout_factory = Arc::new(layout_thread_2020::LayoutFactoryImpl());
        let initial_state = InitialConstellationState {
            compositor_proxy: compositor_sender.clone(),
            embedder_proxy: embedder_sender,
            devtools_sender,
            bluetooth_thread,
            font_cache_thread,
            public_resource_threads,
            private_resource_threads,
            time_profiler_chan: time_profiler_sender.clone(),
            mem_profiler_chan: mem_profiler_sender.clone(),
            webrender_document,
            webrender_api_sender,
            webxr_registry: webxr_registry.registry(),
            webgl_threads: Some(webgl_threads),
            glplayer_threads: None,
            player_context: glplayer_context,
            user_agent,
            webrender_external_images: external_images,
            wgpu_image_map,
        };

        // Create constellation thread
        let constellation_sender = Constellation::<
            script::script_thread::ScriptThread,
            script::serviceworker_manager::ServiceWorkerManager,
        >::start(
            initial_state,
            layout_factory,
            window_size,
            opts.random_pipeline_closure_probability,
            opts.random_pipeline_closure_seed,
            opts.hard_fail,
            !opts.debug.disable_canvas_antialiasing,
            canvas_create_sender,
            canvas_ipc_sender,
        );

        // Create webdriver thread
        if let Some(port) = opts.webdriver_port {
            webdriver_server::start_server(port, constellation_sender.clone());
        }

        let composite_target = if let Some(path) = opts.output_file.clone() {
            CompositeTarget::PngFile(path.into())
        } else {
            CompositeTarget::Window
        };

        // The compositor coordinates with the client window to create the final
        // rendered page and display it somewhere.
        let compositor = IOCompositor::create(
            window.gl_window(),
            InitialCompositorState {
                sender: compositor_sender,
                receiver: compositor_receiver,
                constellation_chan: constellation_sender.clone(),
                time_profiler_chan: time_profiler_sender,
                mem_profiler_chan: mem_profiler_sender,
                webrender,
                webrender_document,
                webrender_api,
                rendering_context,
                webrender_gl,
                webxr_main_thread: webxr_registry,
            },
            composite_target,
            opts.exit_after_load,
            opts.debug.convert_mouse_to_touch,
            top_level_browsing_context_id,
        );

        Verso {
            window,
            webview_id: top_level_browsing_context_id,
            compositor,
            constellation_sender,
            embedder_receiver,
            messages_for_embedder: Vec::new(),
            profiler_enabled: false,
            _js_engine_setup: js_engine_setup,
        }
    }
}

#[derive(Debug, Clone)]
struct Waker(pub EventLoopProxy<()>);

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        if let Err(e) = self.0.send_event(()) {
            log::error!(
                "Servo embedder failed to send wake up event to Verso: {}",
                e
            );
        }
    }
}

fn default_user_agent_string() -> &'static str {
    #[cfg(macos)]
    const UA_STRING: &str =
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:109.0) Servo/1.0 Firefox/111.0";
    #[cfg(ios)]
    const UA_STRING: &str =
        "Mozilla/5.0 (iPhone; CPU iPhone OS 16_4 like Mac OS X; rv:109.0) Servo/1.0 Firefox/111.0";
    #[cfg(android)]
    const UA_STRING: &str = "Mozilla/5.0 (Android; Mobile; rv:109.0) Servo/1.0 Firefox/111.0";
    #[cfg(linux)]
    const UA_STRING: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:109.0) Servo/1.0 Firefox/111.0";
    #[cfg(windows)]
    const UA_STRING: &str =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Servo/1.0 Firefox/111.0";

    UA_STRING
}

#[derive(Clone)]
struct RenderNotifier {
    compositor_proxy: CompositorProxy,
}

impl RenderNotifier {
    pub fn new(compositor_proxy: CompositorProxy) -> RenderNotifier {
        RenderNotifier { compositor_proxy }
    }
}

impl webrender::api::RenderNotifier for RenderNotifier {
    fn clone(&self) -> Box<dyn webrender::api::RenderNotifier> {
        Box::new(RenderNotifier::new(self.compositor_proxy.clone()))
    }

    fn wake_up(&self, _composite_needed: bool) {}

    fn new_frame_ready(
        &self,
        _document_id: DocumentId,
        _scrolled: bool,
        composite_needed: bool,
        _frame_publish_id: FramePublishId,
    ) {
        self.compositor_proxy
            .send(CompositorMsg::NewWebRenderFrameReady(composite_needed));
    }
}

struct WebRenderFontApiCompositorProxy(CompositorProxy);

impl WebRenderFontApi for WebRenderFontApiCompositorProxy {
    fn add_font_instance(
        &self,
        font_key: FontKey,
        size: f32,
        flags: FontInstanceFlags,
    ) -> FontInstanceKey {
        let (sender, receiver) = unbounded();
        self.0
            .send(CompositorMsg::Forwarded(ForwardedToCompositorMsg::Font(
                FontToCompositorMsg::AddFontInstance(font_key, size, flags, sender),
            )));
        receiver.recv().unwrap()
    }

    fn add_font(&self, data: Arc<Vec<u8>>, index: u32) -> FontKey {
        let (sender, receiver) = unbounded();
        let (bytes_sender, bytes_receiver) =
            ipc::bytes_channel().expect("failed to create IPC channel");
        self.0
            .send(CompositorMsg::Forwarded(ForwardedToCompositorMsg::Font(
                FontToCompositorMsg::AddFont(sender, index, bytes_receiver),
            )));
        let _ = bytes_sender.send(&data);
        receiver.recv().unwrap()
    }

    fn add_system_font(&self, handle: NativeFontHandle) -> FontKey {
        let (sender, receiver) = unbounded();
        self.0
            .send(CompositorMsg::Forwarded(ForwardedToCompositorMsg::Font(
                FontToCompositorMsg::AddSystemFont(sender, handle),
            )));
        receiver.recv().unwrap()
    }

    fn forward_add_font_message(
        &self,
        bytes_receiver: ipc::IpcBytesReceiver,
        font_index: u32,
        result_sender: IpcSender<FontKey>,
    ) {
        let (sender, receiver) = unbounded();
        self.0
            .send(CompositorMsg::Forwarded(ForwardedToCompositorMsg::Font(
                FontToCompositorMsg::AddFont(sender, font_index, bytes_receiver),
            )));
        let _ = result_sender.send(receiver.recv().unwrap());
    }

    fn forward_add_font_instance_message(
        &self,
        font_key: FontKey,
        size: f32,
        flags: FontInstanceFlags,
        result_sender: IpcSender<FontInstanceKey>,
    ) {
        let (sender, receiver) = unbounded();
        self.0
            .send(CompositorMsg::Forwarded(ForwardedToCompositorMsg::Font(
                FontToCompositorMsg::AddFontInstance(font_key, size, flags, sender),
            )));
        let _ = result_sender.send(receiver.recv().unwrap());
    }
}

#[derive(Clone)]
struct CanvasWebrenderApi(CompositorProxy);

impl canvas_paint_thread::WebrenderApi for CanvasWebrenderApi {
    fn generate_key(&self) -> Option<ImageKey> {
        let (sender, receiver) = unbounded();
        self.0
            .send(CompositorMsg::Forwarded(ForwardedToCompositorMsg::Canvas(
                CanvasToCompositorMsg::GenerateKey(sender),
            )));
        receiver.recv().ok()
    }
    fn update_images(&self, updates: Vec<ImageUpdate>) {
        self.0
            .send(CompositorMsg::Forwarded(ForwardedToCompositorMsg::Canvas(
                CanvasToCompositorMsg::UpdateImages(updates),
            )));
    }
    fn clone(&self) -> Box<dyn canvas_paint_thread::WebrenderApi> {
        Box::new(<Self as Clone>::clone(self))
    }
}
