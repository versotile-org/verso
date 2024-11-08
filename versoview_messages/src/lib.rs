use ipc_channel::ipc;
use serde::{Deserialize, Serialize};

pub type OnNavigationStartingPayload = ipc::IpcSender<(url::Url, ipc::IpcSender<bool>)>;

#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ControllerMessage {
    NavigateTo(url::Url),
    OnNavigationStarting(OnNavigationStartingPayload),
}
