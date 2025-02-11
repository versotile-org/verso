use dpi::{Position, Size};
use ipc_channel::ipc;
use serde::{Deserialize, Serialize};

// Can't use `PipelineId` directly or else we need to pull in servo as a dependency
type SerializedPipelineId = Vec<u8>;

/// Message sent from the controller to versoview
#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ToVersoMessage {
    /// Navigate to this URL
    NavigateTo(url::Url),
    /// Register a listener on versoview for getting notified on navigation starting
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
