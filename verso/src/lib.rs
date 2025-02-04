use log::error;
use std::{
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
};
use versoview_messages::{ControllerMessage, VersoMessage};

use ipc_channel::{
    ipc::{IpcOneShotServer, IpcSender},
    router::ROUTER,
};

#[derive(Default)]
struct EventListeners {
    on_navigation_starting: Arc<Mutex<Option<Box<dyn Fn(url::Url) -> bool + Send + 'static>>>>,
}

pub struct VersoviewController {
    sender: IpcSender<ControllerMessage>,
    event_listeners: EventListeners,
}

#[derive(Debug, Default)]
pub struct VersoviewSettings {
    pub with_panel: bool,
}

impl VersoviewController {
    /// Create a new verso instance with settings and get the controller to it
    fn create(
        verso_path: impl AsRef<Path>,
        initial_url: url::Url,
        settings: VersoviewSettings,
    ) -> Self {
        let path = verso_path.as_ref();
        let (server, server_name) = IpcOneShotServer::<VersoMessage>::new().unwrap();
        let mut command = Command::new(path);
        command
            .arg(format!("--ipc-channel={server_name}"))
            .arg(format!("--url={initial_url}"));
        if !settings.with_panel {
            command.arg("--no-panel");
        }
        command.spawn().unwrap();
        let (receiver, message) = server.accept().unwrap();
        let VersoMessage::IpcSender(sender) = message else {
            panic!("The initial message sent from versoview is not a `VersoMessage::IpcSender`")
        };
        let event_listeners = EventListeners::default();
        let on_navigation_starting = event_listeners.on_navigation_starting.clone();
        let send_clone = sender.clone();
        ROUTER.add_typed_route(
            receiver,
            Box::new(move |message| match message {
                Ok(message) => match message {
                    VersoMessage::OnNavigationStarting(id, url) => {
                        if let Some(ref callback) = *on_navigation_starting.lock().unwrap() {
                            if let Err(error) = send_clone.send(
                                ControllerMessage::OnNavigationStartingResponse(id, callback(url)),
                            ) {
                                error!(
                                    "Error while sending back OnNavigationStarting result: {error}"
                                );
                            }
                        }
                    }
                    _ => {}
                },
                Err(e) => error!("Error while receiving VersoMessage: {e}"),
            }),
        );
        Self {
            sender,
            event_listeners,
        }
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
        self.sender.send(ControllerMessage::NavigateTo(url))
    }

    /// Listen on navigation starting triggered by user click on a link,
    /// return a boolean in the callback to decide whether or not allowing this navigation
    pub fn on_navigation_starting(
        &self,
        callback: impl Fn(url::Url) -> bool + Send + 'static,
    ) -> Result<(), Box<ipc_channel::ErrorKind>> {
        if self
            .event_listeners
            .on_navigation_starting
            .lock()
            .unwrap()
            .replace(Box::new(callback))
            .is_some()
        {
            return Ok(());
        }
        self.sender
            .send(ControllerMessage::ListenToOnNavigationStarting)?;
        Ok(())
    }
}
