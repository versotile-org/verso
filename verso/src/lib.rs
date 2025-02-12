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
    size_response: Arc<Mutex<Option<std::sync::mpsc::Sender<PhysicalSize<u32>>>>>,
    position_response: Arc<Mutex<Option<std::sync::mpsc::Sender<PhysicalPosition<i32>>>>>,
    maximized_response: Arc<Mutex<Option<std::sync::mpsc::Sender<bool>>>>,
    minimized_response: Arc<Mutex<Option<std::sync::mpsc::Sender<bool>>>>,
    fullscreen_response: Arc<Mutex<Option<std::sync::mpsc::Sender<bool>>>>,
    visible_response: Arc<Mutex<Option<std::sync::mpsc::Sender<bool>>>>,
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
    pub resources_directory: Option<String>,
    pub userscripts_directory: Option<String>,
    pub devtools_port: Option<u16>,
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

        if let Some(resources_directory) = settings.resources_directory {
            command.arg("--resources");
            command.arg(resources_directory);
        }
        if let Some(userscripts_directory) = settings.userscripts_directory {
            command.arg("--userscripts-directory");
            command.arg(userscripts_directory);
        }
        if let Some(devtools_port) = settings.devtools_port {
            command.arg(format!("--devtools-port={devtools_port}"));
        }

        command.spawn().unwrap();

        let (receiver, message) = server.accept().unwrap();
        let ToControllerMessage::SetToVersoSender(sender) = message else {
            panic!("The initial message sent from versoview is not a `VersoMessage::IpcSender`")
        };

        let event_listeners = EventListeners::default();
        let on_navigation_starting = event_listeners.on_navigation_starting.clone();
        let on_web_resource_requested = event_listeners.on_web_resource_requested.clone();
        let size_response = event_listeners.size_response.clone();
        let position_response = event_listeners.position_response.clone();
        let minimized_response = event_listeners.minimized_response.clone();
        let maximized_response = event_listeners.maximized_response.clone();
        let fullscreen_response = event_listeners.fullscreen_response.clone();
        let visible_response = event_listeners.visible_response.clone();
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
                    ToControllerMessage::GetSizeResponse(size) => {
                        if let Some(sender) = size_response.lock().unwrap().take() {
                            sender.send(size).unwrap();
                        }
                    }
                    ToControllerMessage::GetPositionResponse(position) => {
                        if let Some(sender) = position_response.lock().unwrap().take() {
                            sender.send(position).unwrap();
                        }
                    }
                    ToControllerMessage::GetMaximizedResponse(maximized) => {
                        if let Some(sender) = maximized_response.lock().unwrap().take() {
                            sender.send(maximized).unwrap();
                        }
                    }
                    ToControllerMessage::GetMinimizedResponse(minimized) => {
                        if let Some(sender) = minimized_response.lock().unwrap().take() {
                            sender.send(minimized).unwrap();
                        }
                    }
                    ToControllerMessage::GetFullscreenResponse(fullscreen) => {
                        if let Some(sender) = fullscreen_response.lock().unwrap().take() {
                            sender.send(fullscreen).unwrap();
                        }
                    }
                    ToControllerMessage::GetVisibleResponse(visible) => {
                        if let Some(sender) = visible_response.lock().unwrap().take() {
                            sender.send(visible).unwrap();
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

    /// Exit
    pub fn exit(&self) -> Result<(), Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::Exit)
    }

    /// Execute script
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

    /// Get the window's size
    pub fn get_size(&self) -> Result<PhysicalSize<u32>, Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::GetSize)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        self.event_listeners
            .size_response
            .lock()
            .unwrap()
            .replace(sender);
        Ok(receiver.recv().unwrap())
    }

    /// Get the window's position
    pub fn get_position(&self) -> Result<PhysicalPosition<i32>, Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::GetPosition)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        self.event_listeners
            .position_response
            .lock()
            .unwrap()
            .replace(sender);
        Ok(receiver.recv().unwrap())
    }

    /// Get if the window is currently maximized or not
    pub fn is_maximized(&self) -> Result<bool, Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::GetPosition)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        self.event_listeners
            .maximized_response
            .lock()
            .unwrap()
            .replace(sender);
        Ok(receiver.recv().unwrap())
    }

    /// Get if the window is currently minimized or not
    pub fn is_minimized(&self) -> Result<bool, Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::GetMinimized)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        self.event_listeners
            .minimized_response
            .lock()
            .unwrap()
            .replace(sender);
        Ok(receiver.recv().unwrap())
    }

    /// Get if the window is currently fullscreen or not
    pub fn is_fullscreen(&self) -> Result<bool, Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::GetFullscreen)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        self.event_listeners
            .fullscreen_response
            .lock()
            .unwrap()
            .replace(sender);
        Ok(receiver.recv().unwrap())
    }

    /// Get the visibility of the window
    pub fn is_visible(&self) -> Result<bool, Box<ipc_channel::ErrorKind>> {
        self.sender.send(ToVersoMessage::GetVisible)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        self.event_listeners
            .visible_response
            .lock()
            .unwrap()
            .replace(sender);
        Ok(receiver.recv().unwrap())
    }

    // /// Add init script to run on document started to load
    // pub fn add_init_script(&self, script: String) -> Result<(), Box<ipc_channel::ErrorKind>> {
    //     self.sender.send(ToVersoMessage::AddInitScript(script))
    // }
}

impl Drop for VersoviewController {
    fn drop(&mut self) {
        let _ = dbg!(self.exit());
    }
}
