use std::{
    borrow::Cow,
    sync::{atomic::Ordering, Arc},
};

use arboard::Clipboard;
use compositing_traits::{
    CompositorMsg, CompositorProxy, CompositorReceiver, ConstellationMsg, ForwardedToCompositorMsg,
};
use crossbeam_channel::{unbounded, Sender};
use log::{Log, Metadata, Record};
use servo::{
    bluetooth::BluetoothThreadFactory,
    bluetooth_traits::BluetoothRequest,
    canvas::{
        canvas_paint_thread::{self, CanvasPaintThread},
        WebGLComm,
    },
    compositing::{
        windowing::WindowMethods, CompositeTarget, IOCompositor, InitialCompositorState,
        ShutdownState,
    },
    config::{opts, pref},
    constellation::{Constellation, FromCompositorLogger, InitialConstellationState},
    devtools,
    embedder_traits::{EmbedderProxy, EmbedderReceiver, EventLoopWaker},
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
    style,
    url::ServoUrl,
    webgpu,
    webrender_traits::*,
};
use webrender::{api::*, ShaderPrecacheFlags};
use winit::{event::Event, event_loop::EventLoopProxy, window::Window as WinitWindow};

use crate::{
    config::Config,
    window::{GLWindow, Window},
};

/// Main entry point of Verso browser.
pub struct Verso {
    window: Window,
    compositor: Option<IOCompositor<GLWindow>>,
    constellation_sender: Sender<ConstellationMsg>,
    embedder_receiver: EmbedderReceiver,
    /// For single-process Servo instances, this field controls the initialization
    /// and deinitialization of the JS Engine. Multiprocess Servo instances have their
    /// own instance that exists in the content process instead.
    _js_engine_setup: Option<JSEngineSetup>,
    clipboard: Clipboard,
}

impl Verso {
    /// Create a Verso instance from Winit's window and event loop proxy.
    ///
    /// // TODO list the flag to toggle them and ways to disable by default
    /// Following threads will be created while initializing Verso:
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
        // Initialize configurations and Verso window
        let path = config.resource_dir.join("panel.html");
        let url = ServoUrl::from_file_path(path.to_str().unwrap()).unwrap();
        config.init();
        let window = Window::new(window);
        let event_loop_waker = Box::new(Waker(proxy));
        let opts = opts::get();

        // Set Stylo flags
        style::context::DEFAULT_DISABLE_STYLE_SHARING_CACHE
            .store(opts.debug.disable_share_style_cache, Ordering::Relaxed);
        style::context::DEFAULT_DUMP_STYLE_STATISTICS
            .store(opts.debug.dump_style_statistics, Ordering::Relaxed);
        style::traversal::IS_SERVO_NONINCREMENTAL_LAYOUT
            .store(opts.nonincremental_layout, Ordering::Relaxed);

        // Initialize servo media with dummy backend
        servo_media::ServoMedia::init::<servo_media_dummy::DummyBackend>();

        // Initialize surfman & get GL bindings
        let rendering_context = window.rendering_context();
        let webrender_gl = window.webrender_gl.clone();
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

        // Create profiler threads
        let time_profiler_sender = profile::time::Profiler::create(
            &opts.time_profiling,
            opts.time_profiler_trace_path.clone(),
        );
        let mem_profiler_sender = profile::mem::Profiler::create(opts.mem_profiler_period);

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

        // Initialize js engine if it's single process mode
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

        // Create bluetooth thread
        let bluetooth_thread: IpcSender<BluetoothRequest> =
            BluetoothThreadFactory::new(embedder_sender.clone());

        // Create resource thread pool
        let user_agent: Cow<'static, str> = default_user_agent_string().into();
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

        // Create layout factory
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

        // The division by 1 represents the page's default zoom of 100%,
        // and gives us the appropriate CSSPixel type for the viewport.
        let window_size = WindowSizeData {
            initial_viewport: viewport_size / Scale::new(1.0),
            device_pixel_ratio: Scale::new(device_pixel_ratio),
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
        let panel_id = window.panel.id();
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
            panel_id,
        );

        // Create Verso instance
        let verso = Verso {
            window,
            compositor: Some(compositor),
            constellation_sender,
            embedder_receiver,
            _js_engine_setup: js_engine_setup,
            clipboard: Clipboard::new()
                .expect("Clipboard isn't supported in this platform or desktop environment."),
        };

        // Send the constellation message to start Panel UI
        send_to_constellation(
            &verso.constellation_sender,
            ConstellationMsg::NewWebView(url, panel_id),
        );
        verso.setup_logging();

        verso
    }

    /// Run an iteration of Verso handling cycle. An iteration will perform following actions:
    ///
    /// - Handle Winit's event, updating Compositor and sending messages to Constellation.
    /// - Handle Servo's messages and updating Compositor again.
    pub fn run(&mut self, event: Event<()>) {
        self.handle_winit_event(event);
        self.handle_servo_messages();
    }

    /// Handle Winit events
    fn handle_winit_event(&mut self, event: Event<()>) {
        log::trace!("Verso is handling Winit event: {event:?}");
        match event {
            Event::Suspended | Event::Resumed | Event::UserEvent(()) => {}
            Event::WindowEvent {
                window_id: _,
                event,
            } => {
                if let Some(compositor) = &mut self.compositor {
                    self.window.handle_winit_window_event(
                        &self.constellation_sender,
                        compositor,
                        &event,
                    )
                }
            }
            e => log::warn!("Verso isn't supporting this event yet: {e:?}"),
        }
    }

    /// Handle message came from Servo.
    fn handle_servo_messages(&mut self) {
        let mut shutdown = false;
        if let Some(compositor) = &mut self.compositor {
            // Handle Compositor's messages first
            log::trace!("Verso is handling Compositor messages");
            if compositor.receive_messages() {
                // And then handle Embedder messages
                log::trace!(
                    "Verso is handling Embedder messages when shutdown state is set to {:?}",
                    compositor.shutdown_state
                );
                while let Some((top_level_browsing_context, msg)) =
                    self.embedder_receiver.try_recv_embedder_msg()
                {
                    match compositor.shutdown_state {
                        ShutdownState::NotShuttingDown => {
                            // TODO we need to worry about which window to handle message in
                            // multiwindow
                            self.window.handle_servo_message(
                                top_level_browsing_context,
                                msg,
                                &self.constellation_sender,
                                compositor,
                                &mut self.clipboard,
                            );
                        }
                        ShutdownState::FinishedShuttingDown => {
                            log::error!("Verso shouldn't be handling messages after compositor has shut down");
                        }
                        ShutdownState::ShuttingDown => {}
                    }
                }
            }

            if compositor.shutdown_state != ShutdownState::FinishedShuttingDown {
                // Update compositor
                compositor.perform_updates();
            } else {
                shutdown = true;
            }
        }

        if shutdown {
            // If Compositor has shut down, deinit and remove it.
            self.compositor.take().map(IOCompositor::deinit);
        }
    }

    /// Return true if one of the Verso windows is animating.
    pub fn is_animating(&self) -> bool {
        self.window.is_animating()
    }

    /// Return true if Verso has shut down and hence there's no compositor.
    pub fn finished_shutting_down(&self) -> bool {
        self.compositor.is_none()
    }

    fn setup_logging(&self) {
        let constellation_chan = self.constellation_sender.clone();
        let env = env_logger::Env::default();
        let env_logger = env_logger::Builder::from_env(env).build();
        let con_logger = FromCompositorLogger::new(constellation_chan);

        let filter = std::cmp::max(env_logger.filter(), con_logger.filter());
        let logger = BothLogger(env_logger, con_logger);

        log::set_boxed_logger(Box::new(logger)).expect("Failed to set logger.");
        log::set_max_level(filter);
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
            log::error!("Servo failed to send wake up event to Verso: {}", e);
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

// A logger that logs to two downstream loggers.
// This should probably be in the log crate.
struct BothLogger<Log1, Log2>(Log1, Log2);

impl<Log1, Log2> Log for BothLogger<Log1, Log2>
where
    Log1: Log,
    Log2: Log,
{
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.0.enabled(metadata) || self.1.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        self.0.log(record);
        self.1.log(record);
    }

    fn flush(&self) {
        self.0.flush();
        self.1.flush();
    }
}

pub(crate) fn send_to_constellation(sender: &Sender<ConstellationMsg>, msg: ConstellationMsg) {
    let variant_name = msg.variant_name();
    if let Err(e) = sender.send(msg) {
        log::warn!("Sending {variant_name} to constellation failed: {e:?}");
    }
}
