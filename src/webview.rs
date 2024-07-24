use std::cell::Cell;

use arboard::Clipboard;
use compositing_traits::ConstellationMsg;
use crossbeam_channel::Sender;
use servo::{
    base::id::{PipelineNamespace, PipelineNamespaceId, WebViewId},
    embedder_traits::{CompositorEventVariant, EmbedderMsg, PromptDefinition},
    euclid::Size2D,
    script_traits::TraversalDirection,
    url::ServoUrl,
    webrender_api::units::DeviceIntRect,
    TopLevelBrowsingContextId,
};

use crate::{
    compositor::{webview::UnknownWebView, IOCompositor},
    verso::send_to_constellation,
    window::{GLWindow, Window},
};

/// A web view is an area to display web browsing context. It's what user will treat as a "web page".
pub struct WebView {
    id: WebViewId,
    history: Cell<Vec<ServoUrl>>,
    current: Cell<usize>,
}

impl WebView {
    /// Create a web view from Winit window.
    pub fn new(id: WebViewId) -> Self {
        Self {
            id,
            history: Cell::new(vec![]),
            current: Cell::new(0),
        }
    }

    /// Get web view ID of this window.
    pub fn id(&self) -> WebViewId {
        self.id
    }

    /// Set the history URLs and current index of the WebView
    pub fn set_history(&self, history: Vec<ServoUrl>, current: usize) {
        self.history.replace(history);
        self.current.replace(current);
    }
}

impl Window {
    /// Handle servo messages with corresponding web view ID.
    pub fn handle_servo_messages_with_webview(
        &mut self,
        webview_id: WebViewId,
        message: EmbedderMsg,
        sender: &Sender<ConstellationMsg>,
        compositor: &mut IOCompositor<GLWindow>,
        clipboard: &mut Clipboard,
    ) {
        log::trace!("Verso WebView {webview_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::LoadStart | EmbedderMsg::HeadParsed => {}
            EmbedderMsg::LoadComplete => {
                self.window.request_redraw();
            }
            EmbedderMsg::WebViewOpened(w) => {
                let webview = WebView::new(w);
                self.webview = Some(webview);

                let size = self.window.inner_size();
                let size = Size2D::new(size.width as i32, size.height as i32);
                let mut rect = DeviceIntRect::from_size(size).to_f32();
                rect.min.y = rect.max.y.min(76.);
                send_to_constellation(sender, ConstellationMsg::FocusWebView(w));
                compositor.move_resize_webview(w, rect);
            }
            EmbedderMsg::AllowNavigationRequest(id, _url) => {
                // TODO should provide a API for users to check url
                send_to_constellation(sender, ConstellationMsg::AllowNavigationResponse(id, true));
            }
            EmbedderMsg::WebViewClosed(_w) => {
                self.webview = None;
            }
            EmbedderMsg::WebViewFocused(w) => {
                if let Err(UnknownWebView(webview_id)) = compositor.show_webview(w, false) {
                    log::warn!("{webview_id}: ShowWebView on unknown webview id");
                }
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
                    if let Err(UnknownWebView(webview_id)) =
                        compositor.raise_webview_to_top(webview_id, false)
                    {
                        log::warn!("{webview_id}: RaiseWebViewToTop on unknown webview id");
                    }
                    send_to_constellation(sender, ConstellationMsg::FocusWebView(webview_id));
                }
            }
            e => {
                log::warn!("Verso WebView isn't supporting this message yet: {e:?}")
            }
        }
    }
}

/// A panel is a special web view that focus on controlling states around window.
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
pub struct Panel {
    id: WebViewId,
}

impl Panel {
    /// Create a panel from Winit window.
    pub fn new() -> Self {
        // Reserving a namespace to create TopLevelBrowsingContextId.
        PipelineNamespace::install(PipelineNamespaceId(0));
        let id = TopLevelBrowsingContextId::new();
        Self { id }
    }

    /// Get web view ID of this panel.
    ///
    /// We assume this is always called after `set_id`. Calling before it will cause panic.
    pub fn id(&self) -> WebViewId {
        self.id
    }
}

impl Window {
    /// Handle servo messages with main panel.
    pub fn handle_servo_messages_with_panel(
        &mut self,
        panel_id: WebViewId,
        message: EmbedderMsg,
        sender: &Sender<ConstellationMsg>,
        compositor: &mut IOCompositor<GLWindow>,
        clipboard: &mut Clipboard,
    ) {
        log::trace!("Verso Panel {panel_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::LoadStart | EmbedderMsg::HeadParsed => {}
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
            EmbedderMsg::WebViewOpened(w) => {
                let size = self.window.inner_size();
                let size = Size2D::new(size.width as i32, size.height as i32);
                let rect = DeviceIntRect::from_size(size).to_f32();
                send_to_constellation(sender, ConstellationMsg::FocusWebView(w));
                compositor.move_resize_webview(w, rect);
            }
            EmbedderMsg::WebViewClosed(_w) => {
                compositor.maybe_start_shutting_down();
            }
            EmbedderMsg::WebViewFocused(w) => {
                if let Err(UnknownWebView(webview_id)) = compositor.show_webview(w, false) {
                    log::warn!("{webview_id}: ShowWebView on unknown webview id");
                }
            }
            EmbedderMsg::HistoryChanged(..) | EmbedderMsg::ChangePageTitle(..) => {
                log::trace!("Verso Panel ignores this message: {message:?}")
            }
            EmbedderMsg::Prompt(definition, _origin) => {
                match definition {
                    PromptDefinition::Input(msg, _, prompt_sender) => {
                        if let Some(webview) = &self.webview {
                            let id = webview.id();

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
