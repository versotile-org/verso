use std::{fs, path::PathBuf};

use embedder_traits::resources::{self, Resource, ResourceReaderMethods};
use headers::{ContentType, HeaderMapExt};
use net::protocols::{ProtocolHandler, ProtocolRegistry};
use net_traits::{
    request::Request,
    response::{Response, ResponseBody},
    ResourceFetchTiming,
};
use servo_config::{
    opts::{set_options, Opts, OutputOptions},
    prefs::Preferences,
};
use winit::{dpi, window::WindowAttributes};

/// Servo time profile settings
#[derive(Clone, Debug)]
pub struct ProfilerSettings {
    /// Servo time profile settings
    output_options: OutputOptions,
    /// When servo profiler is enabled, this is an optional path to dump a self-contained HTML file
    /// visualizing the traces as a timeline.
    trace_path: Option<String>,
}

/// Command line arguments.
#[derive(Clone, Debug, Default)]
pub struct CliArgs {
    /// URL to load initially.
    pub url: Option<url::Url>,
    /// The IPC channel name used to communicate with the webview controller.
    pub ipc_channel: Option<String>,
    /// Should launch without control panel
    pub no_panel: bool,
    /// Window settings for the initial winit window
    pub window_attributes: WindowAttributes,
    /// Port number to start a server to listen to remote Firefox devtools connections. 0 for random port.
    pub devtools_port: Option<u16>,
    /// Servo time profile settings
    pub profiler_settings: Option<ProfilerSettings>,
    /// Path to resource directory. If None, Verso will try to get default directory. And if that
    /// still doesn't exist, all resource configuration will set to default values.
    pub resource_dir: Option<PathBuf>,
    /// Override the user agent
    pub user_agent: Option<String>,
    /// Script to run on document started to load
    pub init_script: Option<String>,
    /// The directory to load userscripts from
    pub userscripts_directory: Option<String>,
    /// Initial window's zoom level
    pub zoom_level: Option<f32>,
}

/// Configuration of Verso instance.
#[derive(Clone, Debug)]
pub struct Config {
    /// Global flag options of Servo.
    pub opts: Opts,
    /// Command line arguments.
    pub args: CliArgs,
    /// Path to resource directory. If None, Verso will try to get default directory. And if that
    /// still doesn't exist, all resource configuration will set to default values.
    pub resource_dir: PathBuf,
}

fn parse_cli_args() -> Result<CliArgs, getopts::Fail> {
    let args: Vec<String> = std::env::args().collect();

    let mut opts = getopts::Options::new();
    opts.optopt("", "url", "URL to load on start", "docs.rs");
    opts.optopt("", "resources", "Path to resource directory", "PATH");
    opts.optopt(
        "",
        "ipc-channel",
        "IPC channel name to communicate and control verso",
        "",
    );
    opts.optflag("", "no-panel", "Launch Verso without control panel");
    opts.optopt(
        "",
        "devtools-port",
        "Launch Verso with devtools server enabled and listen to port",
        "1234",
    );
    opts.optopt(
        "p",
        "profiler",
        "Launch Verso with servo time profiler enabled and output to stdout with an interval",
        "5",
    );
    opts.optopt(
        "",
        "profiler-output-file",
        "Make servo profiler output to this file instead of stdout",
        "out.tsv",
    );
    opts.optopt(
        "",
        "profiler-trace-path",
        "Path to dump a self-contained HTML timeline of profiler traces",
        "out.html",
    );

    opts.optopt(
        "",
        "user-agent",
        "Override the user agent",
        "'VersoView/1.0'",
    );
    opts.optopt(
        "",
        "init-script",
        "Script to run on document started to load",
        "console.log('hello world')",
    );
    opts.optopt(
        "",
        "userscripts-directory",
        "The directory to load userscripts from",
        "resources/user-agent-js/",
    );

    opts.optopt(
        "w",
        "width",
        "Initial window's width in physical unit, the height command line arg must also be set",
        "1280",
    );
    opts.optopt(
        "h",
        "height",
        "Initial window's height in physical unit, the width command line arg must also be set",
        "720",
    );
    opts.optopt(
        "x",
        "",
        "Initial window's top left x position in physical unit, the y command line arg must also be set. Wayland isn't supported.",
        "200",
    );
    opts.optopt(
        "y",
        "",
        "Initial window's top left y position in physical unit, the x command line arg must also be set. Wayland isn't supported.",
        "200",
    );
    opts.optflag(
        "",
        "no-maximized",
        "Launch the initial window without maximized",
    );

    opts.optopt("", "zoom", "Initial window's zoom level", "1.5");

    let matches: getopts::Matches = opts.parse(&args[1..])?;
    let url = matches
        .opt_str("url")
        .and_then(|url| match url::Url::parse(&url) {
            Ok(url_parsed) => Some(url_parsed),
            Err(e) => {
                if e == url::ParseError::RelativeUrlWithoutBase {
                    if let Ok(url_parsed) = url::Url::parse(&format!("https://{url}")) {
                        return Some(url_parsed);
                    }
                }
                log::error!("Invalid initial url: {url}");
                None
            }
        });
    let resource_dir = matches.opt_str("resources").map(PathBuf::from);
    let ipc_channel = matches.opt_str("ipc-channel");
    let no_panel = matches.opt_present("no-panel");
    let devtools_port = matches.opt_get::<u16>("devtools-port").unwrap_or_else(|e| {
        log::error!("Failed to parse devtools-port command line argument: {e}");
        None
    });

    let profiler_settings = if let Ok(Some(profiler_interval)) = matches.opt_get("profiler") {
        let profile_output = matches.opt_str("profiler-output-file");
        let trace_output = matches.opt_str("profiler-trace-path");
        Some(ProfilerSettings {
            output_options: if let Some(output_file) = profile_output {
                OutputOptions::FileName(output_file)
            } else {
                OutputOptions::Stdout(profiler_interval)
            },
            trace_path: trace_output,
        })
    } else {
        None
    };

    let user_agent = matches.opt_str("user-agent");
    let init_script = matches.opt_str("init-script");
    let userscripts_directory = matches.opt_str("userscripts-directory");

    let mut window_attributes = winit::window::Window::default_attributes();

    // set min inner size
    // should be at least able to show the whole control panel
    // FIXME: url input has weird behavior that will expand lager when having long text
    if !no_panel {
        window_attributes = window_attributes.with_min_inner_size(dpi::LogicalSize::new(480, 72));
    }

    let width = matches.opt_get::<u32>("width").unwrap_or_else(|e| {
        log::error!("Failed to parse width command line argument: {e}");
        None
    });
    let height = matches.opt_get::<u32>("height").unwrap_or_else(|e| {
        log::error!("Failed to parse height command line argument: {e}");
        None
    });
    match (width, height) {
        (Some(_width), None) => {
            log::error!("Invalid size command line argument, width is present but not height");
        }
        (None, Some(_height)) => {
            log::error!("Invalid size command line argument, height is present but not width");
        }
        (Some(width), Some(height)) => {
            window_attributes =
                window_attributes.with_inner_size(dpi::PhysicalSize::new(width, height))
        }
        _ => {}
    };

    let x = matches.opt_get::<u32>("x").unwrap_or_else(|e| {
        log::error!("Failed to parse x command line argument: {e}");
        None
    });
    let y = matches.opt_get::<u32>("y").unwrap_or_else(|e| {
        log::error!("Failed to parse y command line argument: {e}");
        None
    });
    match (x, y) {
        (Some(_x), None) => {
            log::error!("Invalid size command line argument, x is present but not y");
        }
        (None, Some(_y)) => {
            log::error!("Invalid size command line argument, y is present but not x");
        }
        (Some(x), Some(y)) => {
            window_attributes = window_attributes.with_position(dpi::PhysicalPosition::new(x, y))
        }
        _ => {}
    };

    if !matches.opt_present("no-maximized") {
        window_attributes = window_attributes.with_maximized(true);
    }

    let zoom_level = matches.opt_get::<f32>("zoom").unwrap_or_else(|e| {
        log::error!("Failed to parse zoom command line argument: {e}");
        None
    });

    Ok(CliArgs {
        url,
        resource_dir,
        ipc_channel,
        no_panel,
        window_attributes,
        devtools_port,
        profiler_settings,
        user_agent,
        init_script,
        userscripts_directory,
        zoom_level,
    })
}

impl Config {
    /// Create a new configuration for creating Verso instance.
    pub fn new() -> Self {
        let mut opts = Opts::default();
        let args = parse_cli_args().unwrap_or_default();

        let (devtools_server_enabled, devtools_port) =
            if let Some(devtools_port) = args.devtools_port {
                (true, devtools_port)
            } else {
                (false, 0)
            };

        servo_config::prefs::set(Preferences {
            devtools_server_enabled,
            devtools_server_port: devtools_port as i64,
            ..Default::default()
        });

        if let Some(ref profiler_settings) = args.profiler_settings {
            opts.time_profiling = Some(profiler_settings.output_options.clone());
            opts.time_profiler_trace_path = profiler_settings.trace_path.clone();
        }

        if let Some(ref userscripts_directory) = args.userscripts_directory {
            opts.userscripts = Some(userscripts_directory.clone());
        }

        let resource_dir = args.resource_dir.clone().unwrap_or(resources_dir_path());

        Self {
            opts,
            args,
            resource_dir,
        }
    }

    /// Register URL scheme protocols
    pub fn create_protocols(&self) -> ProtocolRegistry {
        let handler = ResourceReader(self.resource_dir.clone());
        let mut protocols = ProtocolRegistry::with_internal_protocols();
        protocols.register("verso", handler);
        protocols
    }

    /// Init options and preferences.
    pub fn init(self) {
        // Set the resource files and preferences of Servo.
        resources::set(Box::new(ResourceReader(self.resource_dir)));

        // Set the global options of Servo.
        set_options(self.opts);
    }
}

struct ResourceReader(PathBuf);

impl ResourceReaderMethods for ResourceReader {
    fn read(&self, file: Resource) -> Vec<u8> {
        let path = self.0.join(file.filename());
        // Rigppy image is the only one needs to be valid bytes.
        // Others can be empty and Servo will set to default.
        if let Resource::RippyPNG = file {
            fs::read(path).unwrap_or(include_bytes!("../resources/rippy.png").to_vec())
        } else {
            fs::read(path).unwrap_or_default()
        }
    }

    fn sandbox_access_files(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn sandbox_access_files_dirs(&self) -> Vec<PathBuf> {
        vec![]
    }
}

impl ProtocolHandler for ResourceReader {
    fn load(
        &self,
        request: &mut Request,
        _done_chan: &mut net::fetch::methods::DoneChannel,
        _context: &net::fetch::methods::FetchContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
        let current_url = request.current_url();
        let path = current_url.path();
        let path = self.0.join(path.strip_prefix('/').unwrap_or(path));

        let response = if let Ok(file) = fs::read(path.clone()) {
            let mut response = Response::new(
                request.current_url(),
                ResourceFetchTiming::new(request.timing_type()),
            );

            // Set Content-Type header.
            if let Some(ext) = path.extension() {
                match ext.to_str() {
                    Some("css") => response
                        .headers
                        .typed_insert(ContentType::from(mime::TEXT_CSS)),
                    Some("js") => response
                        .headers
                        .typed_insert(ContentType::from(mime::TEXT_JAVASCRIPT)),
                    Some("json") => response.headers.typed_insert(ContentType::json()),
                    Some("html") => response.headers.typed_insert(ContentType::html()),
                    _ => response.headers.typed_insert(ContentType::octet_stream()),
                }
            }

            *response.body.lock().unwrap() = ResponseBody::Done(file);

            response
        } else {
            Response::network_internal_error("Opening file failed")
        };

        Box::pin(std::future::ready(response))
    }
}

/// Helper function to get default resource directory if it's not provided.
fn resources_dir_path() -> PathBuf {
    #[cfg(feature = "packager")]
    let root_dir = {
        use cargo_packager_resource_resolver::{current_format, resources_dir};
        current_format().and_then(|format| resources_dir(format))
    };
    #[cfg(feature = "flatpak")]
    let root_dir = {
        use std::str::FromStr;
        std::path::PathBuf::from_str("/app")
    };
    #[cfg(not(any(feature = "packager", feature = "flatpak")))]
    let root_dir = std::env::current_dir();

    root_dir.ok().map(|dir| dir.join("resources")).unwrap()
}
