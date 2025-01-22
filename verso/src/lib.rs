use std::{path::Path, process::Command};
use versoview_messages::ControllerMessage;

use ipc_channel::ipc::{IpcOneShotServer, IpcSender};

#[derive(Debug, Default)]
pub struct VersoviewSettings {
    pub with_panel: bool,
}

pub struct VersoviewController(IpcSender<ControllerMessage>);

impl VersoviewController {
    /// Create a new verso instance with settings and get the controller to it
    fn create(
        verso_path: impl AsRef<Path>,
        initial_url: url::Url,
        settings: VersoviewSettings,
    ) -> Self {
        let path = verso_path.as_ref();
        let (server, server_name) =
            IpcOneShotServer::<IpcSender<ControllerMessage>>::new().unwrap();
        let mut command = Command::new(path);
        command
            .arg(format!("--ipc-channel={server_name}"))
            .arg(format!("--url={initial_url}"));
        if !settings.with_panel {
            command.arg("--no-panel");
        }
        command.spawn().unwrap();
        let (_, sender) = server.accept().unwrap();
        Self(sender)
    }

    /// Create a new verso instance with default settings and get the controller to it
    pub fn new(verso_path: impl AsRef<Path>, initial_url: url::Url) -> Self {
        Self::create(verso_path, initial_url, VersoviewSettings::default())
    }

    /// Create a new verso instance with custom settings and get the controller to it
    pub fn new_with_settings(
        verso_path: impl AsRef<Path>,
        initial_url: url::Url,
        settings: VersoviewSettings,
    ) -> Self {
        Self::create(verso_path, initial_url, settings)
    }

    /// Navigate to url
    pub fn navigate(&self, url: url::Url) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.0.send(ControllerMessage::NavigateTo(url))
    }
}
