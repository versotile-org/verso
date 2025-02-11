use dpi::{PhysicalPosition, PhysicalSize, Position, Size};
use log::error;
use std::{
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
};
use versoview_messages::{
    ToControllerMessage, ToVersoMessage, WebResourceRequest, WebResourceRequestResponse,
};

use ipc_channel::{
    ipc::{IpcOneShotServer, IpcSender},
    router::ROUTER,
};

type ResponseFunction = Box<dyn FnOnce(Option<http::Response<Vec<u8>>>) + Send>;

#[derive(Default)]
struct EventListeners {
    on_navigation_starting: Arc<Mutex<Option<Box<dyn Fn(url::Url) -> bool + Send + 'static>>>>,
    on_web_resource_requested:
        Arc<Mutex<Option<Box<dyn Fn(WebResourceRequest, ResponseFunction) + Send + 'static>>>>,
}

pub struct VersoviewController {
    sender: IpcSender<ToVersoMessage>,
    event_listeners: EventListeners,
}

#[derive(Debug, Default)]
pub struct VersoviewSettings {
    pub with_panel: bool,
    pub size: Option<PhysicalSize<u32>>,
    pub position: Option<PhysicalPosition<i32>>,
    pub maximized: bool,
}

impl VersoviewController {
    /// Create a new verso instance with settings and get the controller to it
    fn create(
        verso_path: impl AsRef<Path>,
        initial_url: url::Url,
        settings: VersoviewSettings,
    ) -> Self {
        let path = verso_path.as_ref();
        let (server, server_name) = IpcOneShotServer::<ToControllerMessage>::new().unwrap();
        let mut command = Command::new(path);
        command
            .arg(format!("--ipc-channel={server_name}"))
            .arg(format!("--url={initial_url}"));
        if !settings.with_panel {
            command.arg("--no-panel");
        }

        if let Some(size) = settings.size {
            let width = size.width;
            let height = size.height;
            command.arg(format!("--width={width}"));
            command.arg(format!("--height={height}"));
        }
        if let Some(position) = settings.position {
            let x = position.x;
            let y = position.y;
            command.arg(format!("--x={x}"));
            command.arg(format!("--y={y}"));
        }
        if !settings.maximized {
            command.arg("--no-maximized");
        }

        command.spawn().unwrap();

        let (receiver, message) = server.accept().unwrap();
        let ToControllerMessage::SetToVersoSender(sender) = message else {
            panic!("The initial message sent from versoview is not a `VersoMessage::IpcSender`")
        };
        let event_listeners = EventListeners::default();
        let on_navigation_starting = event_listeners.on_navigation_starting.clone();
        let on_web_resource_requested = event_listeners.on_web_resource_requested.clone();
        let send_clone = sender.clone();
        ROUTER.add_typed_route(
            receiver,
            Box::new(move |message| match message {
                Ok(message) => match message {
                    ToControllerMessage::OnNavigationStarting(id, url) => {
                        if let Some(ref callback) = *on_navigation_starting.lock().unwrap() {
                            if let Err(error) = send_clone.send(
                                ToVersoMessage::OnNavigationStartingResponse(id, callback(url)),
                            ) {
                                error!(
                                    "Error while sending back OnNavigationStarting result: {error}"
                                );
                            }
                        }
                    }
                    ToControllerMessage::OnWebResourceRequested(request) => {
                        if let Some(ref callback) = *on_web_resource_requested.lock().unwrap() {
                            let sender_clone = send_clone.clone();
                            let id = request.id;
                            callback(
                                request,
                                Box::new(move |response| {
                                    if let Err(error) = sender_clone.send(ToVersoMessage::WebResourceRequestResponse(
                                        WebResourceRequestResponse { id, response },
                                    )) {
                                        error!("Error while sending back OnNavigationStarting result: {error}");
                                    }
                                }),
                            );
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

    pub fn execute_script(&self, script: String) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::ExecuteScript(script))
    }

    /// Navigate to url
    pub fn navigate(&self, url: url::Url) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::NavigateTo(url))
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
            .send(ToVersoMessage::ListenToOnNavigationStarting)?;
        Ok(())
    }

    /// Listen on web resource requests,
    /// return a boolean in the callback to decide whether or not allowing this navigation
    pub fn on_web_resource_requested(
        &self,
        callback: impl Fn(WebResourceRequest, ResponseFunction) + Send + 'static,
    ) -> Result<(), Box<ipc_channel::ErrorKind>> {
        if self
            .event_listeners
            .on_web_resource_requested
            .lock()
            .unwrap()
            .replace(Box::new(callback))
            .is_some()
        {
            return Ok(());
        }
        self.sender
            .send(ToVersoMessage::ListenToWebResourceRequests)?;
        Ok(())
    }

    /// Sets the webview window's size
    pub fn set_size<S: Into<Size>>(&self, size: S) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::SetSize(size.into()))?;
        Ok(())
    }

    /// Sets the webview window's position
    pub fn set_position<P: Into<Position>>(
        &self,
        position: P,
    ) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.sender
            .send(ToVersoMessage::SetPosition(position.into()))?;
        Ok(())
    }

    /// Maximize or unmaximize the window
    pub fn set_maximized(&self, maximized: bool) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::SetMaximized(maximized))?;
        Ok(())
    }

    /// Minimize or unminimize the window
    pub fn set_minimized(&self, minimized: bool) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::SetMinimized(minimized))?;
        Ok(())
    }

    /// Sets the window to fullscreen or back
    pub fn set_fullscreen(&self, fullscreen: bool) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.sender
            .send(ToVersoMessage::SetFullscreen(fullscreen))?;
        Ok(())
    }

    /// Show or hide the window
    pub fn set_visible(&self, visible: bool) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::SetVisible(visible))?;
        Ok(())
    }
}
