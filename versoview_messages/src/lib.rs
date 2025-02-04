use ipc_channel::ipc;
use serde::{Deserialize, Serialize};

// Can't use `PipelineId` directly or else we need to pull in servo as a dependency
type SerializedPipelineId = Vec<u8>;

/// Message sent from the controller to versoview
#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ControllerMessage {
    NavigateTo(url::Url),
    ListenToOnNavigationStarting,
    OnNavigationStartingResponse(SerializedPipelineId, bool),
}

/// Message sent from versoview to the controller
#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum VersoMessage {
    IpcSender(ipc::IpcSender<ControllerMessage>),
    OnNavigationStarting(SerializedPipelineId, url::Url),
}
