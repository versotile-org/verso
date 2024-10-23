use std::{path::Path, process::Command};
use versoview_messages::ControllerMessage;

use ipc_channel::ipc::{IpcOneShotServer, IpcSender};

pub fn run_versoview(
    verso_path: impl AsRef<Path>,
    initial_url: url::Url,
) -> IpcSender<ControllerMessage> {
    let path = verso_path.as_ref();
    let (server, server_name) = IpcOneShotServer::<IpcSender<ControllerMessage>>::new().unwrap();
    Command::new(path)
        .arg(format!("--ipc-channel={server_name}"))
        .arg(format!("--url={initial_url}"))
        .spawn()
        .unwrap();
    let (_, sender) = server.accept().unwrap();
    sender
}
