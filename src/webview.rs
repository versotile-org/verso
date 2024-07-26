use arboard::Clipboard;
use compositing_traits::ConstellationMsg;
use crossbeam_channel::Sender;
use servo::{
    base::id::{PipelineId, PipelineNamespace, PipelineNamespaceId, WebViewId},
    embedder_traits::{CompositorEventVariant, EmbedderMsg, PromptDefinition},
    script_traits::TraversalDirection,
    url::ServoUrl,
    webrender_api::units::DeviceIntRect,
    TopLevelBrowsingContextId,
};

use crate::{verso::send_to_constellation, window::Window};

/// A web view is an area to display web browsing context. It's what user will treat as a "web page".
#[derive(Debug, Clone)]
pub struct WebView {
    webview_id: WebViewId,
    pub pipeline_id: Option<PipelineId>,
    pub rect: DeviceIntRect,
}

impl WebView {
    /// Create a web view from Winit window.
    pub fn new(webview_id: WebViewId, rect: DeviceIntRect) -> Self {
        Self {
            webview_id,
            pipeline_id: None,
            rect,
        }
    }

    /// Create a panel view from Winit window. A panel is a special web view that focus on controlling states around window.
    /// It could be treated as the control panel or navigation bar of the window depending on usages.
    ///
    /// At the moment, following Web API is supported:
    /// - Close window: `window.close()`
    /// - Navigate to previous page: `window.prompt('PREV')`
    /// - Navigate to next page: `window.prompt('FORWARD')`
    /// - Refresh the page: `window.prompt('REFRESH')`
    /// - Minimize the window: `window.prompt('MINIMIZE')`
    /// - Maximize the window: `window.prompt('MAXIMIZE')`
    /// - Navigate to a specific URL: `window.prompt('NAVIGATE_TO:${url}')`
    pub fn new_panel(rect: DeviceIntRect) -> Self {
        // Reserving a namespace to create TopLevelBrowsingContextId.
        PipelineNamespace::install(PipelineNamespaceId(0));
        let id = TopLevelBrowsingContextId::new();
        Self {
            webview_id: id,
            pipeline_id: None,
            rect,
        }
    }

    /// Get web view ID of this window.
    pub fn webview_id(&self) -> WebViewId {
        self.webview_id
    }
}

impl Window {
    /// Handle servo messages with corresponding web view ID.
    pub fn handle_servo_messages_with_webview(
        &mut self,
        webview_id: WebViewId,
        message: EmbedderMsg,
        sender: &Sender<ConstellationMsg>,
        clipboard: &mut Clipboard,
    ) {
        log::trace!("Verso WebView {webview_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::LoadStart
            | EmbedderMsg::HeadParsed
            | EmbedderMsg::WebViewOpened(_)
            | EmbedderMsg::WebViewClosed(_)
            | EmbedderMsg::WebViewFocused(_) => {
                // Most WebView messages are ignored because it's done by compositor.
                log::trace!("Verso WebView {webview_id:?} ignores this message: {message:?}")
            }
            EmbedderMsg::LoadComplete => {
                self.window.request_redraw();
            }
            EmbedderMsg::AllowNavigationRequest(id, _url) => {
                // TODO should provide a API for users to check url
                send_to_constellation(sender, ConstellationMsg::AllowNavigationResponse(id, true));
            }
            EmbedderMsg::GetClipboardContents(sender) => {
                let contents = clipboard.get_text().unwrap_or_else(|e| {
                    log::warn!(
                        "Verso WebView {webview_id:?} failed to get clipboard content: {}",
                        e
                    );
                    String::new()
                });
                if let Err(e) = sender.send(contents) {
                    log::warn!(
                        "Verso WebView {webview_id:?} failed to send clipboard content: {}",
                        e
                    );
                }
            }
            EmbedderMsg::SetClipboardContents(text) => {
                if let Err(e) = clipboard.set_text(text) {
                    log::warn!(
                        "Verso WebView {webview_id:?} failed to set clipboard contents: {}",
                        e
                    );
                }
            }
            EmbedderMsg::EventDelivered(event) => {
                if let CompositorEventVariant::MouseButtonEvent = event {
                    send_to_constellation(sender, ConstellationMsg::FocusWebView(webview_id));
                }
            }
            e => {
                log::warn!("Verso WebView isn't supporting this message yet: {e:?}")
            }
        }
    }

    /// Handle servo messages with main panel.
    pub fn handle_servo_messages_with_panel(
        &mut self,
        panel_id: WebViewId,
        message: EmbedderMsg,
        sender: &Sender<ConstellationMsg>,
        clipboard: &mut Clipboard,
    ) {
        log::trace!("Verso Panel {panel_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::LoadStart
            | EmbedderMsg::HeadParsed
            | EmbedderMsg::WebViewOpened(_)
            | EmbedderMsg::WebViewClosed(_)
            | EmbedderMsg::WebViewFocused(_) => {
                // Most WebView messages are ignored because it's done by compositor.
                log::trace!("Verso Panel ignores this message: {message:?}")
            }
            EmbedderMsg::LoadComplete => {
                self.window.request_redraw();
                // let demo_url = ServoUrl::parse("https://demo.versotile.org").unwrap();
                let demo_url = ServoUrl::parse("https://keyboard-test.space").unwrap();
                let demo_id = TopLevelBrowsingContextId::new();
                send_to_constellation(sender, ConstellationMsg::NewWebView(demo_url, demo_id));
            }
            EmbedderMsg::AllowNavigationRequest(id, _url) => {
                // The panel shouldn't navigate to other pages.
                send_to_constellation(sender, ConstellationMsg::AllowNavigationResponse(id, false));
            }
            EmbedderMsg::HistoryChanged(..) | EmbedderMsg::ChangePageTitle(..) => {
                log::trace!("Verso Panel ignores this message: {message:?}")
            }
            EmbedderMsg::Prompt(definition, _origin) => {
                match definition {
                    PromptDefinition::Input(msg, _, prompt_sender) => {
                        if let Some(webview) = &self.webview {
                            let id = webview.webview_id();

                            if msg.starts_with("NAVIGATE_TO:") {
                                let url =
                                    ServoUrl::parse(msg.strip_prefix("NAVIGATE_TO:").unwrap())
                                        .unwrap();
                                send_to_constellation(sender, ConstellationMsg::LoadUrl(id, url));
                            } else {
                                match msg.as_str() {
                                    "PREV" => {
                                        send_to_constellation(
                                            sender,
                                            ConstellationMsg::TraverseHistory(
                                                id,
                                                TraversalDirection::Back(1),
                                            ),
                                        );
                                        // TODO Set EmbedderMsg::Status to None
                                    }
                                    "FORWARD" => {
                                        send_to_constellation(
                                            sender,
                                            ConstellationMsg::TraverseHistory(
                                                id,
                                                TraversalDirection::Forward(1),
                                            ),
                                        );
                                        // TODO Set EmbedderMsg::Status to None
                                    }
                                    "REFRESH" => {
                                        send_to_constellation(sender, ConstellationMsg::Reload(id));
                                    }
                                    "MINIMIZE" => {
                                        self.window.set_minimized(true);
                                    }
                                    "MAXIMIZE" => {
                                        let is_maximized = self.window.is_maximized();
                                        self.window.set_maximized(!is_maximized);
                                    }
                                    e => log::warn!(
                                        "Verso Panel isn't supporting this prompt message yet: {e}"
                                    ),
                                }
                            }
                        }
                        let _ = prompt_sender.send(None);
                    }
                    _ => log::warn!("Verso Panel isn't supporting this prompt yet"),
                }
            }
            EmbedderMsg::GetClipboardContents(sender) => {
                let contents = clipboard.get_text().unwrap_or_else(|e| {
                    log::warn!("Verso Panel failed to get clipboard content: {}", e);
                    String::new()
                });
                if let Err(e) = sender.send(contents) {
                    log::warn!("Verso Panel failed to send clipboard content: {}", e);
                }
            }
            EmbedderMsg::SetClipboardContents(text) => {
                if let Err(e) = clipboard.set_text(text) {
                    log::warn!("Verso Panel failed to set clipboard contents: {}", e);
                }
            }
            e => {
                log::warn!("Verso Panel isn't supporting this message yet: {e:?}")
            }
        }
    }
}
