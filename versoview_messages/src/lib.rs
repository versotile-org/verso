use std::path::PathBuf;

use dpi::{PhysicalPosition, PhysicalSize, Position, Size};
use ipc_channel::ipc;
use serde::{Deserialize, Serialize};

// Note: the reason why we didn't send `IpcSender` in those messages is because it panics on MacOS,
// see https://github.com/versotile-org/verso/pull/222#discussion_r1939111585,
// the work around is let verso send back the message through the initial sender and we map them back manually

// Can't use `PipelineId` directly or else we need to pull in servo as a dependency
type SerializedPipelineId = Vec<u8>;

/// Message sent from the controller to versoview
#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ToVersoMessage {
    /// Initial configs for versoview
    /// this will be the first message sent to Verso once we received the sender from [`ToControllerMessage::SetToVersoSender`]
    SetConfig(ConfigFromController),
    /// Exit
    Exit,
    /// Register a listener on versoview for getting notified on close requested from the OS,
    /// veroview will send a [`ToControllerMessage::OnCloseRequested`] when that happens
    ListenToOnCloseRequested,
    /// Navigate to this URL
    NavigateTo(url::Url),
    /// Register a listener on versoview for getting notified on navigation starting,
    /// veroview will send a [`ToControllerMessage::OnNavigationStarting`] when that happens
    ListenToOnNavigationStarting,
    /// Response to a [`ToControllerMessage::OnNavigationStarting`] message from versoview
    OnNavigationStartingResponse(SerializedPipelineId, bool),
    /// Execute JavaScript
    ExecuteScript(String),
    /// Register a listener on versoview for getting notified on web resource requests
    ListenToWebResourceRequests,
    /// Response to a [`ToControllerMessage::OnWebResourceRequested`] message from versoview
    WebResourceRequestResponse(WebResourceRequestResponse),
    /// Sets the webview window's size
    SetSize(Size),
    /// Sets the webview window's position
    SetPosition(Position),
    /// Maximize or unmaximize the window
    SetMaximized(bool),
    /// Minimize or unminimize the window
    SetMinimized(bool),
    /// Sets the window to fullscreen or back
    SetFullscreen(bool),
    /// Show or hide the window
    SetVisible(bool),
    /// Moves the window with the left mouse button until the button is released
    StartDragging,
    /// Get the window's size, need a response with [`ToControllerMessage::GetSizeResponse`]
    GetSize(uuid::Uuid),
    /// Get the window's position, need a response with [`ToControllerMessage::GetPositionResponse`]
    GetPosition(uuid::Uuid),
    /// Get if the window is currently maximized or not, need a response with [`ToControllerMessage::GetMaximizedResponse`]
    GetMaximized(uuid::Uuid),
    /// Get if the window is currently minimized or not, need a response with [`ToControllerMessage::GetMinimizedResponse`]
    GetMinimized(uuid::Uuid),
    /// Get if the window is currently fullscreen or not, need a response with [`ToControllerMessage::GetFullscreenResponse`]
    GetFullscreen(uuid::Uuid),
    /// Get the visibility of the window, need a response with [`ToControllerMessage::GetVisibleResponse`]
    GetVisible(uuid::Uuid),
    /// Get the scale factor of the window, need a response with [`ToControllerMessage::GetScaleFactorResponse`]
    GetScaleFactor(uuid::Uuid),
    /// Get the current URL of the webview, need a response with [`ToControllerMessage::GetCurrentUrlResponse`]
    GetCurrentUrl(uuid::Uuid),
}

/// Message sent from versoview to the controller
#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ToControllerMessage {
    /// IPC sender for the controller to send commands to versoview,
    /// this will be the first message sent to the controller once connected
    SetToVersoSender(ipc::IpcSender<ToVersoMessage>),
    /// Sent on a new navigation starting, need a response with [`ToVersoMessage::OnNavigationStartingResponse`]
    OnNavigationStarting(SerializedPipelineId, url::Url),
    /// Sent on a new web resource request, need a response with [`ToVersoMessage::WebResourceRequestResponse`]
    OnWebResourceRequested(WebResourceRequest),
    /// Response to a [`ToVersoMessage::GetSize`]
    GetSizeResponse(uuid::Uuid, PhysicalSize<u32>),
    /// Response to a [`ToVersoMessage::GetPosition`]
    GetPositionResponse(uuid::Uuid, Option<PhysicalPosition<i32>>),
    /// Response to a [`ToVersoMessage::GetMaximized`]
    GetMaximizedResponse(uuid::Uuid, bool),
    /// Response to a [`ToVersoMessage::GetMinimized`]
    GetMinimizedResponse(uuid::Uuid, bool),
    /// Response to a [`ToVersoMessage::GetFullscreen`]
    GetFullscreenResponse(uuid::Uuid, bool),
    /// Response to a [`ToVersoMessage::GetVisible`]
    GetVisibleResponse(uuid::Uuid, bool),
    /// Response to a [`ToVersoMessage::GetScaleFactor`]
    GetScaleFactorResponse(uuid::Uuid, f64),
    /// Response to a [`ToVersoMessage::GetCurrentUrl`]
    GetCurrentUrlResponse(uuid::Uuid, url::Url),
    /// Verso have recieved a close request from the OS
    OnCloseRequested,
}

/// Configuration of Verso instance.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigFromController {
    /// URL to load initially.
    pub url: Option<url::Url>,
    /// Should launch without or without control panel
    pub with_panel: bool,
    /// Window size for the initial winit window
    pub inner_size: Option<Size>,
    /// Window position for the initial winit window
    pub position: Option<Position>,
    /// Launch maximized or not for the initial winit window
    pub maximized: bool,
    /// Launch visible or not for the initial winit window
    pub visible: bool,
    /// Launch fullscreen or not for the initial winit window
    pub fullscreen: bool,
    /// Port number to start a server to listen to remote Firefox devtools connections. 0 for random port.
    pub devtools_port: Option<u16>,
    /// Servo time profile settings
    pub profiler_settings: Option<ProfilerSettings>,
    /// Override the user agent
    pub user_agent: Option<String>,
    /// Script to run on document started to load
    pub init_script: Option<String>,
    /// The directory to load userscripts from
    pub userscripts_directory: Option<String>,
    /// Initial window's zoom level
    pub zoom_level: Option<f32>,
    /// Path to resource directory. If None, Verso will try to get default directory. And if that
    /// still doesn't exist, all resource configuration will set to default values.
    pub resources_directory: Option<PathBuf>,
}

impl Default for ConfigFromController {
    fn default() -> Self {
        Self {
            url: None,
            with_panel: false,
            inner_size: None,
            position: None,
            maximized: false,
            visible: true,
            fullscreen: false,
            devtools_port: None,
            profiler_settings: None,
            user_agent: None,
            init_script: None,
            userscripts_directory: None,
            zoom_level: None,
            resources_directory: None,
        }
    }
}

/// Servo time profile settings
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProfilerSettings {
    /// Servo time profile settings
    pub output_options: OutputOptions,
    /// When servo profiler is enabled, this is an optional path to dump a self-contained HTML file
    /// visualizing the traces as a timeline.
    pub trace_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum OutputOptions {
    /// Database connection config (hostname, name, user, pass)
    FileName(String),
    Stdout(f64),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebResourceRequest {
    pub id: uuid::Uuid,
    #[serde(with = "http_serde_ext::request")]
    pub request: http::Request<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebResourceRequestResponse {
    pub id: uuid::Uuid,
    #[serde(with = "http_serde_ext::response::option")]
    pub response: Option<http::Response<Vec<u8>>>,
}
