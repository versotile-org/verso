use std::{
    fs,
    path::{Path, PathBuf},
};

use dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use embedder_traits::resources::{self, Resource, ResourceReaderMethods};
use embedder_traits::user_content_manager::UserScript as ServoUserScript;
use headers::{ContentType, HeaderMapExt};
use net::protocols::{ProtocolHandler, ProtocolRegistry};
use net_traits::{
    ResourceFetchTiming,
    request::Request,
    response::{Response, ResponseBody},
};
use servo_config::{
    opts::{Opts, OutputOptions, set_options},
    prefs::Preferences,
};
use versoview_messages::{ConfigFromController, UserScript};
use winit::window::{Fullscreen, WindowAttributes};

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
    /// Window size for the initial window
    pub inner_size: Option<PhysicalSize<u32>>,
    /// Window position for the initial window
    pub position: Option<PhysicalPosition<i32>>,
    /// Don't maximize the initial window
    pub no_maximized: bool,
    /// Port number to start a server to listen to remote Firefox devtools connections. 0 for random port.
    pub devtools_port: Option<u16>,
    /// Servo time profile settings
    pub profiler_settings: Option<versoview_messages::ProfilerSettings>,
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

/// Parse CLI arguments to a [`CliArgs`]
pub fn parse_cli_args() -> Result<CliArgs, getopts::Fail> {
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
        Some(versoview_messages::ProfilerSettings {
            output_options: if let Some(output_file) = profile_output {
                versoview_messages::OutputOptions::FileName(output_file)
            } else {
                versoview_messages::OutputOptions::Stdout(profiler_interval)
            },
            trace_path: trace_output,
        })
    } else {
        None
    };

    let user_agent = matches.opt_str("user-agent");
    let init_script = matches.opt_str("init-script");
    let userscripts_directory = matches.opt_str("userscripts-directory");

    let width = matches.opt_get::<u32>("width").unwrap_or_else(|e| {
        log::error!("Failed to parse width command line argument: {e}");
        None
    });
    let height = matches.opt_get::<u32>("height").unwrap_or_else(|e| {
        log::error!("Failed to parse height command line argument: {e}");
        None
    });
    let inner_size = match (width, height) {
        (Some(_width), None) => {
            log::error!("Invalid size command line argument, width is present but not height");
            None
        }
        (None, Some(_height)) => {
            log::error!("Invalid size command line argument, height is present but not width");
            None
        }
        (Some(width), Some(height)) => Some(PhysicalSize::new(width, height)),
        _ => None,
    };

    let x = matches.opt_get::<i32>("x").unwrap_or_else(|e| {
        log::error!("Failed to parse x command line argument: {e}");
        None
    });
    let y = matches.opt_get::<i32>("y").unwrap_or_else(|e| {
        log::error!("Failed to parse y command line argument: {e}");
        None
    });
    let position = match (x, y) {
        (Some(_x), None) => {
            log::error!("Invalid size command line argument, x is present but not y");
            None
        }
        (None, Some(_y)) => {
            log::error!("Invalid size command line argument, y is present but not x");
            None
        }
        (Some(x), Some(y)) => Some(PhysicalPosition::new(x, y)),
        _ => None,
    };

    let no_maximized = matches.opt_present("no-maximized");

    let zoom_level = matches.opt_get::<f32>("zoom").unwrap_or_else(|e| {
        log::error!("Failed to parse zoom command line argument: {e}");
        None
    });

    Ok(CliArgs {
        url,
        resource_dir,
        ipc_channel,
        no_panel,
        devtools_port,
        profiler_settings,
        user_agent,
        init_script,
        userscripts_directory,
        zoom_level,
        inner_size,
        position,
        no_maximized,
    })
}

/// Configuration of Verso instance.
#[derive(Clone, Debug)]
pub struct Config {
    /// URL to load initially.
    pub url: url::Url,
    /// Should launch without or without control panel
    pub with_panel: bool,
    /// Window settings for the initial winit window
    pub window_attributes: WindowAttributes,
    /// Port number to start a server to listen to remote Firefox devtools connections. 0 for random port.
    pub devtools_port: Option<u16>,
    /// Servo time profile settings
    pub profiler_settings: Option<ProfilerSettings>,
    /// Override the user agent
    pub user_agent: String,
    /// Script to run on document started to load
    pub user_scripts: Vec<ServoUserScript>,
    /// Initial window's zoom level
    pub zoom_level: Option<f32>,
    /// Path to resource directory. If None, Verso will try to get default directory. And if that
    /// still doesn't exist, all resource configuration will set to default values.
    pub resource_dir: PathBuf,
}

impl Config {
    /// Create a new configuration for creating Verso instance from the CLI arguments.
    pub fn from_cli_args(cli_args: CliArgs) -> Self {
        let mut user_scripts = Vec::new();
        if let Some(init_script) = cli_args.init_script {
            user_scripts.push(init_script.into());
        }
        user_scripts.extend(
            load_userscripts(cli_args.userscripts_directory).expect("Failed to load userscript"),
        );
        Self::from_controller_config(ConfigFromController {
            url: cli_args.url,
            with_panel: !cli_args.no_panel,
            devtools_port: cli_args.devtools_port,
            profiler_settings: cli_args.profiler_settings,
            user_agent: cli_args.user_agent,
            user_scripts,
            zoom_level: cli_args.zoom_level,
            resources_directory: cli_args.resource_dir,
            maximized: !cli_args.no_maximized,
            position: cli_args.position.map(Into::into),
            inner_size: cli_args.inner_size.map(Into::into),
            ..Default::default()
        })
    }

    /// Create a new configuration for creating Verso instance from the controller config.
    pub fn from_controller_config(config: ConfigFromController) -> Self {
        let resource_dir = config
            .resources_directory
            .unwrap_or_else(resources_dir_path);
        let with_panel = config.with_panel;
        let user_agent = config
            .user_agent
            .unwrap_or_else(|| default_user_agent_string().to_string());

        let mut window_attributes = winit::window::Window::default_attributes()
            .with_transparent(config.transparent)
            .with_decorations(config.decorated)
            .with_title(config.title.unwrap_or("Verso".to_owned()))
            .with_window_icon(config.icon.and_then(|icon| {
                winit::window::Icon::from_rgba(icon.rgba, icon.width, icon.height).ok()
            }));
        // set min inner size
        // should be at least able to show the whole control panel
        // FIXME: url input has weird behavior that will expand lager when having long text
        if with_panel {
            window_attributes = window_attributes.with_min_inner_size(LogicalSize::new(480, 72));
        }
        if let Some(position) = config.position {
            window_attributes = window_attributes.with_position(position);
        }
        if let Some(size) = config.inner_size {
            window_attributes = window_attributes.with_inner_size(size);
        }
        window_attributes = window_attributes.with_maximized(config.maximized);
        window_attributes = window_attributes.with_fullscreen(if config.fullscreen {
            Some(Fullscreen::Borderless(None))
        } else {
            None
        });
        window_attributes = window_attributes.with_visible(config.visible);
        window_attributes = window_attributes.with_active(config.focused);

        let profiler_settings =
            config
                .profiler_settings
                .map(|profiler_settings| ProfilerSettings {
                    output_options: match profiler_settings.output_options {
                        versoview_messages::OutputOptions::FileName(outfile) => {
                            OutputOptions::FileName(outfile)
                        }
                        versoview_messages::OutputOptions::Stdout(profiler_interval) => {
                            OutputOptions::Stdout(profiler_interval)
                        }
                    },
                    trace_path: profiler_settings.trace_path,
                });

        Self {
            url: config
                .url
                .unwrap_or_else(|| url::Url::parse("https://example.com").unwrap()),
            with_panel,
            window_attributes,
            devtools_port: config.devtools_port,
            profiler_settings,
            user_agent,
            user_scripts: config
                .user_scripts
                .into_iter()
                .map(|userscript| ServoUserScript {
                    script: userscript.script,
                    source_file: userscript.source_file,
                })
                .collect(),
            zoom_level: config.zoom_level,
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
    pub fn init(&self) {
        // Set the resource files of Servo.
        resources::set(Box::new(ResourceReader(self.resource_dir.clone())));

        let mut opts = Opts::default();

        if let Some(ref profiler_settings) = self.profiler_settings {
            opts.time_profiling = Some(profiler_settings.output_options.clone());
            opts.time_profiler_trace_path = profiler_settings.trace_path.clone();
        }

        // Set the global options of Servo.
        set_options(opts);

        let (devtools_server_enabled, devtools_port) =
            if let Some(devtools_port) = self.devtools_port {
                (true, devtools_port)
            } else {
                (false, 0)
            };

        // Set the preferences of Servo.
        servo_config::prefs::set(Preferences {
            dom_svg_enabled: true, // Some pages fail to render if this is disabled
            devtools_server_enabled,
            devtools_server_port: devtools_port as i64,
            dom_notification_enabled: true, // experimental feature
            user_agent: self.user_agent.clone(),
            ..Default::default()
        });
    }
}

fn load_userscripts(
    userscripts_directory: Option<impl AsRef<Path>>,
) -> std::io::Result<Vec<UserScript>> {
    let mut userscripts = Vec::new();
    if let Some(userscripts_directory) = &userscripts_directory {
        let mut files = std::fs::read_dir(userscripts_directory)?
            .map(|e| e.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        files.sort();
        for file in files {
            userscripts.push(UserScript {
                script: std::fs::read_to_string(&file)?,
                source_file: Some(file),
            });
        }
    }
    Ok(userscripts)
}

struct ResourceReader(PathBuf);

impl ResourceReaderMethods for ResourceReader {
    fn read(&self, resource: Resource) -> Vec<u8> {
        let path = self.0.join(resource.filename());
        fs::read(&path).unwrap_or_else(|_| {
            match resource {
                // Rigppy image is the only one needs to be valid bytes.
                // Others can be empty and Servo will set to default.
                Resource::RippyPNG => &include_bytes!("../resources/rippy.png")[..],
                #[cfg(feature = "embed-useragent-stylesheets")]
                Resource::UserAgentCSS => &include_bytes!("../resources/user-agent.css")[..],
                #[cfg(feature = "embed-useragent-stylesheets")]
                Resource::ServoCSS => &include_bytes!("../resources/servo.css")[..],
                #[cfg(feature = "embed-useragent-stylesheets")]
                Resource::PresentationalHintsCSS => {
                    &include_bytes!("../resources/presentational-hints.css")[..]
                }
                Resource::HstsPreloadList => {
                    log::warn!(
                        "HSTS preload list not found, falling back to an empty list, to set this, put the list at '{}'",
                        path.display()
                    );
                    r###"{ "entries": [] }"###.as_bytes()
                }
                _ => &[],
            }
            .to_vec()
        })
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
