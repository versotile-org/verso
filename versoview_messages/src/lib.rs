use ipc_channel::ipc;
use serde::{Deserialize, Serialize};

// Can't use `PipelineId` directly or else we need to pull in servo as a dependency
type SerializedPipelineId = Vec<u8>;

/// Message sent from the controller to versoview
#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ControllerMessage {
    /// Navigate to this URL
    NavigateTo(url::Url),
    /// Register a listener on versoview for getting notified on navigation starting
    ListenToOnNavigationStarting,
    /// Response to a [`VersoMessage::OnNavigationStarting`] message from versoview
    OnNavigationStartingResponse(SerializedPipelineId, bool),
}

/// Message sent from versoview to the controller
#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum VersoMessage {
    /// IPC sender for the controller to send commands to versoview
    IpcSender(ipc::IpcSender<ControllerMessage>),
    /// Sent on a new navigation starting, need a response with [`ControllerMessage::OnNavigationStartingResponse`]
    OnNavigationStarting(SerializedPipelineId, url::Url),
}
