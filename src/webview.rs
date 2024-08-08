use arboard::Clipboard;
use base::id::{PipelineNamespace, PipelineNamespaceId, WebViewId};
use compositing_traits::ConstellationMsg;
use crossbeam_channel::Sender;
use embedder_traits::{CompositorEventVariant, EmbedderMsg, PromptDefinition};
use script_traits::TraversalDirection;
use servo_url::ServoUrl;
use webrender_api::units::DeviceIntRect;

use crate::{compositor::IOCompositor, verso::send_to_constellation, window::Window};

/// A web view is an area to display web browsing context. It's what user will treat as a "web page".
#[derive(Debug, Clone)]
pub struct WebView {
    /// Webview ID
    pub webview_id: WebViewId,
    /// The position and size of the webview.
    pub rect: DeviceIntRect,
}

impl WebView {
    /// Create a web view from Winit window.
    pub fn new(webview_id: WebViewId, rect: DeviceIntRect) -> Self {
        Self { webview_id, rect }
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
        let id = WebViewId::new();
        Self {
            webview_id: id,
            rect,
        }
    }
}

impl Window {
    /// Handle servo messages with corresponding web view ID.
    pub fn handle_servo_messages_with_webview(
        &mut self,
        webview_id: WebViewId,
        message: EmbedderMsg,
        sender: &Sender<ConstellationMsg>,
        clipboard: Option<&mut Clipboard>,
        compositor: &mut IOCompositor,
    ) {
        log::trace!("Verso WebView {webview_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::LoadStart
            | EmbedderMsg::HeadParsed
            | EmbedderMsg::WebViewOpened(_)
            | EmbedderMsg::WebViewClosed(_) => {
                // Most WebView messages are ignored because it's done by compositor.
                log::trace!("Verso WebView {webview_id:?} ignores this message: {message:?}")
            }
            EmbedderMsg::WebViewFocused(w) => {
                compositor.set_webview_loaded(&w);
                compositor.set_painting_order(self);
            }
            EmbedderMsg::LoadComplete => {
                self.window.request_redraw();
                send_to_constellation(sender, ConstellationMsg::FocusWebView(webview_id));
            }
            EmbedderMsg::AllowNavigationRequest(id, _url) => {
                // TODO should provide a API for users to check url
                send_to_constellation(sender, ConstellationMsg::AllowNavigationResponse(id, true));
            }
            EmbedderMsg::GetClipboardContents(sender) => {
                let contents = clipboard
                    .map(|c| {
                        c.get_text().unwrap_or_else(|e| {
                            log::warn!(
                                "Verso WebView {webview_id:?} failed to get clipboard content: {}",
                                e
                            );
                            String::new()
                        })
                    })
                    .unwrap_or_default();
                if let Err(e) = sender.send(contents) {
                    log::warn!(
                        "Verso WebView {webview_id:?} failed to send clipboard content: {}",
                        e
                    );
                }
            }
            EmbedderMsg::SetClipboardContents(text) => {
                clipboard.map(|c| {
                    if let Err(e) = c.set_text(text) {
                        log::warn!(
                            "Verso WebView {webview_id:?} failed to set clipboard contents: {}",
                            e
                        );
                    }
                });
            }
            EmbedderMsg::EventDelivered(event) => {
                if let CompositorEventVariant::MouseButtonEvent = event {
                    send_to_constellation(sender, ConstellationMsg::FocusWebView(webview_id));
                }
            }
            e => {
                log::trace!("Verso WebView isn't supporting this message yet: {e:?}")
            }
        }
    }

    /// Handle servo messages with main panel. Return true it requests a new window.
    pub fn handle_servo_messages_with_panel(
        &mut self,
        panel_id: WebViewId,
        message: EmbedderMsg,
        sender: &Sender<ConstellationMsg>,
        clipboard: Option<&mut Clipboard>,
        compositor: &mut IOCompositor,
    ) -> bool {
        log::trace!("Verso Panel {panel_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::LoadStart
            | EmbedderMsg::HeadParsed
            | EmbedderMsg::WebViewOpened(_)
            | EmbedderMsg::WebViewClosed(_) => {
                // Most WebView messages are ignored because it's done by compositor.
                log::trace!("Verso Panel ignores this message: {message:?}")
            }
            EmbedderMsg::WebViewFocused(w) => {
                compositor.set_webview_loaded(&w);
                compositor.set_painting_order(self);
            }
            EmbedderMsg::LoadComplete => {
                self.window.request_redraw();
                send_to_constellation(sender, ConstellationMsg::FocusWebView(panel_id));

                let demo_url = ServoUrl::parse("https://example.com").unwrap();
                let demo_id = WebViewId::new();
                let size = self.size();
                let mut rect = DeviceIntRect::from_size(size);
                rect.min.y = rect.max.y.min(76);
                self.webview = Some(WebView::new(demo_id, rect));
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
                        let _ = prompt_sender.send(None);
                        if let Some(webview) = &self.webview {
                            let id = webview.webview_id;

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
                                        // send_to_constellation(sender, ConstellationMsg::Reload(id));
                                        return true;
                                    }
                                    "MINIMIZE" => {
                                        self.window.set_minimized(true);
                                    }
                                    "MAXIMIZE" => {
                                        let is_maximized = self.window.is_maximized();
                                        self.window.set_maximized(!is_maximized);
                                    }
                                    e => log::trace!(
                                        "Verso Panel isn't supporting this prompt message yet: {e}"
                                    ),
                                }
                            }
                        }
                    }
                    _ => log::trace!("Verso Panel isn't supporting this prompt yet"),
                }
            }
            EmbedderMsg::GetClipboardContents(sender) => {
                let contents = clipboard
                    .map(|c| {
                        c.get_text().unwrap_or_else(|e| {
                            log::warn!("Verso Panel failed to get clipboard content: {}", e);
                            String::new()
                        })
                    })
                    .unwrap_or_default();
                if let Err(e) = sender.send(contents) {
                    log::warn!("Verso Panel failed to send clipboard content: {}", e);
                }
            }
            EmbedderMsg::SetClipboardContents(text) => {
                clipboard.map(|c| {
                    if let Err(e) = c.set_text(text) {
                        log::warn!("Verso Panel failed to set clipboard contents: {}", e);
                    }
                });
            }
            e => {
                log::trace!("Verso Panel isn't supporting this message yet: {e:?}")
            }
        }
        false
    }
}
