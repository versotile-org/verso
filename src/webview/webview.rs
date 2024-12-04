use arboard::Clipboard;
use base::id::{BrowsingContextId, WebViewId};
use compositing_traits::ConstellationMsg;
use crossbeam_channel::Sender;
use embedder_traits::{
    CompositorEventVariant, EmbedderMsg, PermissionPrompt, PermissionRequest, PromptDefinition,
    PromptResult,
};
use ipc_channel::ipc;
use script_traits::{
    webdriver_msg::{WebDriverJSResult, WebDriverScriptCommand},
    TraversalDirection, WebDriverCommandMsg,
};
use servo_url::ServoUrl;
use url::Url;
use webrender_api::units::DeviceIntRect;

use crate::{
    compositor::IOCompositor,
    verso::send_to_constellation,
    webview::prompt::{PromptDialog, PromptInputResult, PromptSender},
    window::Window,
};

#[cfg(linux)]
use crate::webview::context_menu::ContextMenuResult;

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

    /// Set the webview size.
    pub fn set_size(&mut self, rect: DeviceIntRect) {
        self.rect = rect;
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
    /// The panel's webview
    pub(crate) webview: WebView,
    /// The URL to load when the panel gets loaded
    pub(crate) initial_url: servo_url::ServoUrl,
}

impl Window {
    /// Handle servo messages with corresponding web view ID.
    pub fn handle_servo_messages_with_webview(
        &mut self,
        webview_id: WebViewId,
        message: EmbedderMsg,
        sender: &Sender<ConstellationMsg>,
        clipboard: Option<&mut Clipboard>,
        _compositor: &mut IOCompositor,
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
                self.close_context_menu(sender);
                log::debug!(
                    "Verso Window {:?}'s webview {} has loaded completely.",
                    self.id(),
                    w
                );
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
                if let Some(c) = clipboard {
                    if let Err(e) = c.set_text(text) {
                        log::warn!(
                            "Verso WebView {webview_id:?} failed to set clipboard contents: {}",
                            e
                        );
                    }
                }
            }
            EmbedderMsg::HistoryChanged(list, index) => {
                self.close_prompt_dialog();
                self.update_history(&list, index);
                let url = list.get(index).unwrap();
                if let Some(panel) = self.panel.as_ref() {
                    let (tx, rx) = ipc::channel::<WebDriverJSResult>().unwrap();
                    send_to_constellation(
                        sender,
                        ConstellationMsg::WebDriverCommand(WebDriverCommandMsg::ScriptCommand(
                            BrowsingContextId::from(panel.webview.webview_id),
                            WebDriverScriptCommand::ExecuteScript(
                                format!("window.navbar.setNavbarUrl('{}')", url.as_str()),
                                tx,
                            ),
                        )),
                    );
                    let _ = rx.recv();
                }
            }
            EmbedderMsg::EventDelivered(event) => {
                if let CompositorEventVariant::MouseButtonEvent = event {
                    send_to_constellation(sender, ConstellationMsg::FocusWebView(webview_id));
                }
            }
            EmbedderMsg::ShowContextMenu(_sender, _title, _options) => {
                // TODO: Implement context menu
            }
            EmbedderMsg::Prompt(prompt_type, _origin) => {
                let mut prompt = PromptDialog::new();
                let rect = self.webview.as_ref().unwrap().rect;

                match prompt_type {
                    PromptDefinition::Alert(message, prompt_sender) => {
                        prompt.alert(sender, rect, message, prompt_sender);
                    }
                    PromptDefinition::OkCancel(message, prompt_sender) => {
                        prompt.ok_cancel(sender, rect, message, prompt_sender);
                    }
                    PromptDefinition::YesNo(message, prompt_sender) => {
                        prompt.yes_no(
                            sender,
                            rect,
                            message,
                            PromptSender::ConfirmSender(prompt_sender),
                        );
                    }
                    PromptDefinition::Input(message, default_value, prompt_sender) => {
                        prompt.input(sender, rect, message, Some(default_value), prompt_sender);
                    }
                }

                // save prompt in window to keep prompt_sender alive
                // so that we can send the result back to the prompt after user clicked the button
                self.prompt = Some(prompt);
            }
            EmbedderMsg::PromptPermission(prompt, prompt_sender) => {
                let message = match prompt {
                    PermissionPrompt::Request(permission_name) => {
                        format!(
                            "This website would like to request permission for {:?}.",
                            permission_name
                        )
                    }
                    PermissionPrompt::Insecure(permission_name) => {
                        format!(
                            "This website would like to request permission for {:?}. However current connection is not secure. Do you want to proceed?",
                            permission_name
                        )
                    }
                };

                let mut prompt = PromptDialog::new();
                let rect = self.webview.as_ref().unwrap().rect;
                prompt.yes_no(
                    sender,
                    rect,
                    message,
                    PromptSender::PermissionSender(prompt_sender),
                );

                self.prompt = Some(prompt);
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
        _compositor: &mut IOCompositor,
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
                self.close_context_menu(sender);
                log::debug!(
                    "Verso Window {:?}'s panel {} has loaded completely.",
                    self.id(),
                    w
                );
            }
            EmbedderMsg::LoadComplete => {
                self.window.request_redraw();
                send_to_constellation(sender, ConstellationMsg::FocusWebView(panel_id));

                self.create_webview(sender, self.panel.as_ref().unwrap().initial_url.clone());
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
                                let unparsed_url = msg.strip_prefix("NAVIGATE_TO:").unwrap();
                                let url = match Url::parse(unparsed_url) {
                                    Ok(url_parsed) => url_parsed,
                                    Err(e) => {
                                        if e == url::ParseError::RelativeUrlWithoutBase {
                                            Url::parse(&format!("https://{}", unparsed_url))
                                                .unwrap()
                                        } else {
                                            panic!("Verso Panel failed to parse URL: {}", e);
                                        }
                                    }
                                };

                                send_to_constellation(
                                    sender,
                                    ConstellationMsg::LoadUrl(id, ServoUrl::from_url(url)),
                                );
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
                                    "NEW_WINDOW" => {
                                        return true;
                                    }
                                    "MINIMIZE" => {
                                        self.window.set_minimized(true);
                                    }
                                    "MAXIMIZE" | "DBCLICK_PANEL" => {
                                        let is_maximized = self.window.is_maximized();
                                        self.window.set_maximized(!is_maximized);
                                    }
                                    "DRAG_WINDOW" => {
                                        let _ = self.window.drag_window();
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
                if let Some(c) = clipboard {
                    if let Err(e) = c.set_text(text) {
                        log::warn!("Verso Panel failed to set clipboard contents: {}", e);
                    }
                }
            }
            EmbedderMsg::EventDelivered(event) => {
                if let CompositorEventVariant::MouseButtonEvent = event {
                    send_to_constellation(sender, ConstellationMsg::FocusWebView(panel_id));
                }
            }
            e => {
                log::trace!("Verso Panel isn't supporting this message yet: {e:?}")
            }
        }
        false
    }

    /// Handle servo messages with main panel. Return true it requests a new window.
    #[cfg(linux)]
    pub fn handle_servo_messages_with_context_menu(
        &mut self,
        webview_id: WebViewId,
        message: EmbedderMsg,
        sender: &Sender<ConstellationMsg>,
        _clipboard: Option<&mut Clipboard>,
        _compositor: &mut IOCompositor,
    ) -> bool {
        log::trace!("Verso Context Menu {webview_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::Prompt(definition, _origin) => match definition {
                PromptDefinition::Input(msg, _, prompt_sender) => {
                    let _ = prompt_sender.send(None);
                    if msg.starts_with("CONTEXT_MENU:") {
                        let json_str_msg = msg.strip_prefix("CONTEXT_MENU:").unwrap();
                        let result =
                            serde_json::from_str::<ContextMenuResult>(json_str_msg).unwrap();

                        self.handle_context_menu_event(sender, result);
                    }
                }
                _ => log::trace!("Verso context menu isn't supporting this prompt yet"),
            },
            e => {
                log::trace!("Verso context menu isn't supporting this message yet: {e:?}")
            }
        }
        false
    }

    /// Handle servo messages with prompt. Return true it requests a new window.
    pub fn handle_servo_messages_with_prompt(
        &mut self,
        webview_id: WebViewId,
        message: EmbedderMsg,
        _sender: &Sender<ConstellationMsg>,
        _clipboard: Option<&mut Clipboard>,
        _compositor: &mut IOCompositor,
    ) -> bool {
        log::trace!("Verso Prompt {webview_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::Prompt(prompt, _origin) => match prompt {
                PromptDefinition::Alert(msg, ignored_prompt_sender) => {
                    let prompt = self.prompt.as_ref().unwrap();
                    let prompt_sender = prompt.sender().unwrap();

                    match prompt_sender {
                        PromptSender::AlertSender(sender) => {
                            let _ = sender.send(());
                        }
                        PromptSender::ConfirmSender(sender) => {
                            let result: PromptResult = match msg.as_str() {
                                "ok" | "yes" => PromptResult::Primary,
                                "cancel" | "no" => PromptResult::Secondary,
                                _ => {
                                    log::error!("prompt result message invalid: {msg}");
                                    PromptResult::Dismissed
                                }
                            };
                            let _ = sender.send(result);
                        }
                        PromptSender::InputSender(sender) => {
                            if let Ok(PromptInputResult { action, value }) =
                                serde_json::from_str::<PromptInputResult>(&msg)
                            {
                                match action.as_str() {
                                    "ok" => {
                                        let _ = sender.send(Some(value));
                                    }
                                    "cancel" => {
                                        let _ = sender.send(None);
                                    }
                                    _ => {
                                        log::error!("prompt result message invalid: {msg}");
                                        let _ = sender.send(None);
                                    }
                                }
                            } else {
                                log::error!("prompt result message invalid: {msg}");
                                let _ = sender.send(None);
                            }
                        }
                        PromptSender::PermissionSender(sender) => {
                            let result: PermissionRequest = match msg.as_str() {
                                "ok" | "yes" => PermissionRequest::Granted,
                                "cancel" | "no" => PermissionRequest::Denied,
                                _ => {
                                    log::error!("prompt result message invalid: {msg}");
                                    PermissionRequest::Denied
                                }
                            };
                            let _ = sender.send(result);
                        }
                    }

                    let _ = ignored_prompt_sender.send(());
                }
                _ => {
                    log::trace!("Verso WebView isn't supporting this prompt yet")
                }
            },
            e => {
                log::trace!("Verso Dialog isn't supporting this message yet: {e:?}")
            }
        }
        false
    }
}
