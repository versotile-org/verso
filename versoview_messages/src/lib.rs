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
    GetSize,
    /// Get the window's position, need a response with [`ToControllerMessage::GetPositionResponse`]
    GetPosition,
    /// Get if the window is currently maximized or not, need a response with [`ToControllerMessage::GetMaximizedResponse`]
    GetMaximized,
    /// Get if the window is currently minimized or not, need a response with [`ToControllerMessage::GetMinimizedResponse`]
    GetMinimized,
    /// Get if the window is currently fullscreen or not, need a response with [`ToControllerMessage::GetFullscreenResponse`]
    GetFullscreen,
    /// Get the visibility of the window, need a response with [`ToControllerMessage::GetVisibleResponse`]
    GetVisible,
    /// Get the scale factor of the window, need a response with [`ToControllerMessage::GetScaleFactorResponse`]
    GetScaleFactor,
    /// Get the current URL of the webview, need a response with [`ToControllerMessage::GetCurrentUrlResponse`]
    GetCurrentUrl,
}

/// Message sent from versoview to the controller
#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ToControllerMessage {
    /// IPC sender for the controller to send commands to versoview
    SetToVersoSender(ipc::IpcSender<ToVersoMessage>),
    /// Sent on a new navigation starting, need a response with [`ToVersoMessage::OnNavigationStartingResponse`]
    OnNavigationStarting(SerializedPipelineId, url::Url),
    /// Sent on a new web resource request, need a response with [`ToVersoMessage::WebResourceRequestResponse`]
    OnWebResourceRequested(WebResourceRequest),
    /// Response to a [`ToVersoMessage::GetSize`]
    GetSizeResponse(PhysicalSize<u32>),
    /// Response to a [`ToVersoMessage::GetPosition`]
    GetPositionResponse(PhysicalPosition<i32>),
    /// Response to a [`ToVersoMessage::GetMaximized`]
    GetMaximizedResponse(bool),
    /// Response to a [`ToVersoMessage::GetMinimized`]
    GetMinimizedResponse(bool),
    /// Response to a [`ToVersoMessage::GetFullscreen`]
    GetFullscreenResponse(bool),
    /// Response to a [`ToVersoMessage::GetVisible`]
    GetVisibleResponse(bool),
    /// Response to a [`ToVersoMessage::GetScaleFactor`]
    GetScaleFactorResponse(f64),
    /// Response to a [`ToVersoMessage::GetCurrentUrl`]
    GetCurrentUrlResponse(url::Url),
    /// Verso have recieved a close request from the OS
    OnCloseRequested,
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
