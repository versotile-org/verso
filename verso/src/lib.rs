use std::{path::Path, process::Command};
use versoview_messages::ControllerMessage;

use ipc_channel::ipc::{IpcOneShotServer, IpcSender};

pub struct VersoviewController(IpcSender<ControllerMessage>);

impl VersoviewController {
    /// Create a new verso instance and get the controller to it
    pub fn new(verso_path: impl AsRef<Path>, initial_url: url::Url) -> Self {
        let path = verso_path.as_ref();
        let (server, server_name) =
            IpcOneShotServer::<IpcSender<ControllerMessage>>::new().unwrap();
        Command::new(path)
            .arg(format!("--ipc-channel={server_name}"))
            .arg(format!("--url={initial_url}"))
            .spawn()
            .unwrap();
        let (_, sender) = server.accept().unwrap();
        Self(sender)
    }

    /// Navigate to url
    pub fn navigate(&self, url: url::Url) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.0.send(ControllerMessage::NavigateTo(url))
    }
}
