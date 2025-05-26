use std::str::FromStr;

use arboard::Clipboard;
use base::id::WebViewId;
use constellation_traits::{EmbedderToConstellationMessage, TraversalDirection};
use crossbeam_channel::Sender;
use embedder_traits::{
    AlertResponse, AllowOrDeny, ConfirmResponse, ContextMenuResult, EmbedderMsg, LoadStatus,
    PromptResponse, SimpleDialog, ViewportDetails, WebDriverCommandMsg, WebDriverJSResult,
    WebDriverScriptCommand,
};
use euclid::Scale;
use ipc_channel::ipc::{self, IpcSender};
use servo_url::ServoUrl;
use url::Url;
use versoview_messages::ToControllerMessage;
use webrender_api::units::{DevicePoint, DeviceRect};

use crate::{
    bookmark::{BookmarkId, BookmarkManager},
    compositor::IOCompositor,
    download::{DownloadId, check_should_download, download_body},
    tab::{Tab, TabActivateRequest, TabCloseRequest, TabCreateResponse},
    verso::{VersoInternalMsg, send_to_constellation},
    webview::{
        history_menu::{HistoryMenuUIResponse, OpenHistoryMenuRequest},
        prompt::{HttpBasicAuthInputResult, PromptDialog, PromptInputResult, PromptSender},
    },
    window::Window,
};

#[cfg(linux)]
use crate::webview::context_menu::ContextMenuUIResponse;

/// A web view is an area to display web browsing context. It's what user will treat as a "web page".
#[derive(Debug, Clone)]
pub struct WebView {
    /// Webview ID
    pub webview_id: WebViewId,
    /// The position and size of the webview.
    pub rect: DeviceRect,
}

impl WebView {
    /// Create a web view.
    // TODO: use ViewportDetails instead of hidpi_scale_factor
    pub fn new(webview_id: WebViewId, viewport_details: ViewportDetails) -> Self {
        let size = viewport_details.size * viewport_details.hidpi_scale_factor;
        Self {
            webview_id,
            rect: DeviceRect::from_origin_and_size(DevicePoint::origin(), size),
        }
    }

    /// Set the webview size.
    pub fn set_size(&mut self, rect: DeviceRect) {
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
        sender: &Sender<EmbedderToConstellationMessage>,
        to_controller_sender: &Option<ipc::IpcSender<ToControllerMessage>>,
        clipboard: Option<&mut Clipboard>,
        compositor: &mut IOCompositor,
    ) {
        log::trace!("Verso WebView {webview_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::WebViewClosed(_) => {
                // Most WebView messages are ignored because it's done by compositor.
                log::trace!("Verso WebView {webview_id:?} ignores this message: {message:?}")
            }
            EmbedderMsg::WebViewBlurred => {
                self.focused_webview_id = None;
                self.close_webview_menu(sender);
            }
            EmbedderMsg::WebViewFocused(w) => {
                self.focused_webview_id = Some(webview_id);
                self.close_webview_menu(sender);

                log::debug!(
                    "Verso Window {:?}'s webview {} has loaded completely.",
                    self.id(),
                    w
                );
            }
            EmbedderMsg::NotifyLoadStatusChanged(_webview_id, status) => match status {
                LoadStatus::Complete => {
                    self.window.request_redraw();
                    send_to_constellation(
                        sender,
                        EmbedderToConstellationMessage::FocusWebView(webview_id),
                    );
                }
                _ => {
                    log::trace!(
                        "Verso WebView {webview_id:?} ignores NotifyLoadStatusChanged status: {status:?}"
                    );
                }
            },
            EmbedderMsg::ChangePageTitle(_webview_id, title) => {
                if let Some(panel) = self.panel.as_ref() {
                    let tab = self.tab_manager.current_tab_mut().unwrap();
                    let title = if let Some(title) = title {
                        tab.set_title(title.clone());
                        format!("'{title}'")
                    } else {
                        tab.set_title("null".to_string());
                        "null".to_string()
                    };

                    let script = format!(
                        "window.navbar.setTabTitle('{}', {})",
                        serde_json::to_string(&webview_id).unwrap(),
                        title.as_str()
                    );
                    let _ = execute_script(sender, &panel.webview.webview_id, script);
                }
            }
            EmbedderMsg::AllowNavigationRequest(_webview_id, id, url) => {
                if let Some(to_controller_sender) = to_controller_sender {
                    if self.event_listeners.on_navigation_starting {
                        if let Err(error) =
                            to_controller_sender.send(ToControllerMessage::OnNavigationStarting(
                                bincode::serialize(&id).unwrap(),
                                url.clone().into_url(),
                            ))
                        {
                            log::error!(
                                "Verso failed to send AllowNavigationRequest to controller: {error}"
                            )
                        } else {
                            // We will handle a ToVersoMessage::OnNavigationStartingResponse
                            // and send EmbedderToConstellationMessage::AllowNavigationResponse there if the call succeed
                            return;
                        }
                    }
                }

                // If it's in Verso Browser, check if we should download the url
                if self.panel.is_some() {
                    let sender = sender.clone();
                    let url = url.into_url();
                    let client = self.reqwest_client.clone();
                    let verso_internal_sender = self.verso_internal_sender.clone();

                    tokio::spawn(async move {
                        let (should_download, resp) = check_should_download(&client, &url).await;
                        if should_download && resp.is_some() {
                            download_body(url, resp.unwrap(), verso_internal_sender).await;
                        } else {
                            send_to_constellation(
                                &sender,
                                EmbedderToConstellationMessage::AllowNavigationResponse(id, true),
                            );
                        }
                    });
                } else {
                    // If it's not in Verso Browser, just allow the navigation
                    send_to_constellation(
                        sender,
                        EmbedderToConstellationMessage::AllowNavigationResponse(id, true),
                    );
                }
            }
            EmbedderMsg::WebResourceRequested(_webview_id, request, sender) => {
                if let Some(to_controller_sender) = to_controller_sender {
                    if let Some(request_map) = &mut self.event_listeners.on_web_resource_requested {
                        let id = uuid::Uuid::new_v4();
                        let mut builder = http::request::Builder::new()
                            .uri(request.url.as_str())
                            .method(request.method);
                        for (key, value) in request.headers.iter() {
                            builder = builder.header(key, value);
                        }
                        match to_controller_sender.send(
                            ToControllerMessage::OnWebResourceRequested(
                                versoview_messages::WebResourceRequest {
                                    id,
                                    // TODO: Actually send the body
                                    request: builder.body(Vec::new()).unwrap(),
                                },
                            ),
                        ) {
                            Ok(_) => {
                                request_map.insert(id, (request.url, sender));
                                // We will handle a ToVersoMessage::WebResourceRequestResponse
                                // and send the response through this sender there if the call succeed
                            }
                            Err(error) => {
                                log::error!(
                                    "Verso failed to send WebResourceRequested to controller: {error}"
                                )
                            }
                        }
                    }
                }
            }
            EmbedderMsg::GetClipboardText(_webview_id, sender) => {
                let text = clipboard
                    .map(|c| {
                        c.get_text().unwrap_or_else(|e| {
                            log::warn!(
                                "Verso WebView {webview_id:?} failed to get clipboard text: {}",
                                e
                            );
                            String::new()
                        })
                    })
                    .unwrap_or_default();
                if let Err(e) = sender.send(Ok(text)) {
                    log::warn!(
                        "Verso WebView {webview_id:?} failed to send clipboard text: {}",
                        e
                    );
                }
            }
            EmbedderMsg::SetClipboardText(_webview_id, text) => {
                if let Some(c) = clipboard {
                    if let Err(e) = c.set_text(text) {
                        log::warn!(
                            "Verso WebView {webview_id:?} failed to set clipboard text: {}",
                            e
                        );
                    }
                }
            }
            EmbedderMsg::HistoryChanged(_webview_id, list, index) => {
                self.close_prompt_dialog(webview_id);
                self.close_webview_menu(sender);

                compositor.send_root_pipeline_display_list(self);

                self.tab_manager
                    .set_history(webview_id, list.clone(), index);
                let url = list.get(index).unwrap();
                let prev_btn_enabled = index > 0;
                let next_btn_enabled = index < list.len() - 1;
                if let Some(panel) = self.panel.as_ref() {
                    let _ = execute_script(
                        sender,
                        &panel.webview.webview_id,
                        format!("window.navbar.setNavbarUrl('{}')", url.as_str()),
                    );
                    let _ = execute_script(
                        sender,
                        &panel.webview.webview_id,
                        format!(
                            "window.navbar.setNavBtnEnabled({}, {})",
                            prev_btn_enabled, next_btn_enabled
                        ),
                    );
                }
            }
            EmbedderMsg::ShowContextMenu(_webview_id, servo_sender, _title, _options) => {
                #[cfg(linux)]
                if self.webview_menu.is_none() {
                    self.webview_menu =
                        Some(Box::new(self.show_context_menu(sender, servo_sender)));
                } else {
                    let _ = servo_sender.send(ContextMenuResult::Ignored);
                }
                #[cfg(any(target_os = "windows", target_os = "macos"))]
                {
                    let context_menu = self.show_context_menu(servo_sender);
                    // FIXME: there's chance to lose the event since the channel is async.
                    if let Ok(event) = self.menu_event_receiver.try_recv() {
                        self.handle_context_menu_event(context_menu, sender, event);
                    }
                }
            }
            EmbedderMsg::ShowSimpleDialog(_webview_id, simple_dialog) => {
                if let Some(tab) = self.tab_manager.tab(webview_id) {
                    let mut prompt = PromptDialog::new();
                    let rect = tab.webview().rect;
                    match simple_dialog {
                        SimpleDialog::Alert {
                            message,
                            response_sender,
                        } => {
                            prompt.alert(
                                sender,
                                rect,
                                self.scale_factor() as f32,
                                message,
                                response_sender,
                            );
                        }
                        SimpleDialog::Confirm {
                            message,
                            response_sender,
                        } => {
                            prompt.ok_cancel(
                                sender,
                                rect,
                                self.scale_factor() as f32,
                                message,
                                response_sender,
                            );
                        }
                        SimpleDialog::Prompt {
                            message,
                            default,
                            response_sender,
                        } => {
                            if message.starts_with("VERSO::") {
                                self.handle_verso_internal_messages_with_webview(
                                    &message.strip_prefix("VERSO::").unwrap(),
                                    response_sender,
                                    sender,
                                    tab,
                                );
                                return;
                            }

                            prompt.input(
                                sender,
                                rect,
                                self.scale_factor() as f32,
                                message,
                                Some(default),
                                response_sender,
                            );
                        }
                    }

                    // save prompt in window to keep prompt_sender alive
                    // so that we can send the result back to the prompt after user clicked the button
                    self.tab_manager.set_prompt(webview_id, prompt);
                } else {
                    log::error!("Failed to get WebView {webview_id:?} in this window.");
                }
            }
            EmbedderMsg::PromptPermission(_webview_id, feature, prompt_sender) => {
                if let Some(tab) = self.tab_manager.tab(webview_id) {
                    let message = format!(
                        "This website would like to request permission for {:?}.",
                        feature
                    );

                    let mut prompt = PromptDialog::new();
                    prompt.allow_deny(
                        sender,
                        tab.webview().rect,
                        self.scale_factor() as f32,
                        message,
                        PromptSender::AllowDenySender(prompt_sender),
                    );
                    self.tab_manager.set_prompt(webview_id, prompt);
                } else {
                    log::error!("Failed to get WebView {webview_id:?} in this window.");
                }
            }
            EmbedderMsg::RequestAuthentication(_webview_id, _url, _proxy, response_sender) => {
                if let Some(tab) = self.tab_manager.tab(webview_id) {
                    let mut prompt = PromptDialog::new();
                    let rect = tab.webview().rect;
                    prompt.http_basic_auth(
                        sender,
                        rect,
                        self.scale_factor() as f32,
                        response_sender,
                    );
                    self.tab_manager.set_prompt(webview_id, prompt);
                } else {
                    log::error!("Failed to get WebView {webview_id:?} in this window.");
                }
            }
            EmbedderMsg::SelectFiles(
                _webview_id,
                _filter_pattern,
                _allow_multiple_files,
                ipc_sender,
            ) => {
                if _allow_multiple_files {
                    rfd::FileDialog::new()
                        .pick_files()
                        .map(|files| {
                            if let Err(e) = ipc_sender.send(Some(files)) {
                                log::warn!("Verso Panel failed to send files: {}", e);
                            }
                        })
                        .unwrap_or_else(|| {
                            log::error!("Failed to open file dialog.");
                            if let Err(e) = ipc_sender.send(None) {
                                log::warn!("Verso Panel failed to send files: {}", e);
                            }
                        });
                } else {
                    rfd::FileDialog::new()
                        .pick_file()
                        .map(|file| {
                            if let Err(e) = ipc_sender.send(Some(vec![file])) {
                                log::warn!("Verso Panel failed to send files: {}", e);
                            }
                        })
                        .unwrap_or_else(|| {
                            log::error!("Failed to open file dialog.");
                            if let Err(e) = ipc_sender.send(None) {
                                log::warn!("Verso Panel failed to send files: {}", e);
                            }
                        });
                }
            }
            EmbedderMsg::ShowIME(_webview_id, input_method_type, text, multiline, position) => {
                self.show_ime(
                    input_method_type,
                    text,
                    multiline,
                    position,
                    self.show_bookmark,
                );
            }
            EmbedderMsg::HideIME(_webview_id) => {
                self.hide_ime();
            }
            EmbedderMsg::ShowNotification(_webview_id, notification) => {
                self.show_notification(&notification);
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
        sender: Sender<EmbedderToConstellationMessage>,
        clipboard: Option<&mut Clipboard>,
        compositor: &mut IOCompositor,
        bookmark_manager: &mut BookmarkManager,
    ) -> bool {
        log::trace!("Verso Panel {panel_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::WebViewClosed(_) => {
                // Most WebView messages are ignored because it's done by compositor.
                log::trace!("Verso Panel ignores this message: {message:?}")
            }
            EmbedderMsg::WebViewBlurred => {
                self.focused_webview_id = None;
            }
            EmbedderMsg::WebViewFocused(webview_id) => {
                self.focused_webview_id = Some(webview_id);
                self.close_webview_menu(&sender);
                log::debug!(
                    "Verso Window {:?}'s panel {} has loaded completely.",
                    self.id(),
                    webview_id
                );
            }
            EmbedderMsg::NotifyLoadStatusChanged(_webview_id, status) => {
                if status == LoadStatus::Complete {
                    self.window.request_redraw();
                    send_to_constellation(
                        &sender,
                        EmbedderToConstellationMessage::FocusWebView(panel_id),
                    );

                    self.create_tab(&sender, self.panel.as_ref().unwrap().initial_url.clone());
                } else {
                    log::trace!("Verso Panel ignores NotifyLoadStatusChanged status: {status:?}");
                }
            }
            EmbedderMsg::AllowNavigationRequest(_webview_id, id, _url) => {
                // The panel shouldn't navigate to other pages.
                send_to_constellation(
                    &sender,
                    EmbedderToConstellationMessage::AllowNavigationResponse(id, false),
                );
            }
            EmbedderMsg::HistoryChanged(..) | EmbedderMsg::ChangePageTitle(..) => {
                log::trace!("Verso Panel ignores this message: {message:?}")
            }
            EmbedderMsg::ShowSimpleDialog(_webview_id, simple_dialog) => {
                match simple_dialog {
                    SimpleDialog::Prompt {
                        message,
                        default: _,
                        response_sender,
                    } => {
                        /* Tab */
                        if message.starts_with("CLOSE_TAB:") {
                            let request_str = message.strip_prefix("CLOSE_TAB:").unwrap();
                            let request: TabCloseRequest = serde_json::from_str(request_str)
                                .expect("Failed to parse TabCloseRequest");

                            // close the tab
                            if self.tab_manager.tab(request.id).is_some() {
                                send_to_constellation(
                                    &sender,
                                    EmbedderToConstellationMessage::CloseWebView(request.id),
                                );
                            }

                            let _ = response_sender.send(PromptResponse::default());
                            return false;
                        } else if message.starts_with("ACTIVATE_TAB:") {
                            let request_str = message.strip_prefix("ACTIVATE_TAB:").unwrap();
                            let request: TabActivateRequest = serde_json::from_str(request_str)
                                .expect("Failed to parse TabActivateRequest");

                            let tab_id = request.id;

                            let _ = response_sender.send(PromptResponse::default());

                            // FIXME: set dirty flag, and only resize when flag is set
                            self.activate_tab(compositor, tab_id, self.tab_manager.count() > 1);

                            return false;
                        } else if message == "NEW_TAB" {
                            let hidpi_scale_factor = Scale::new(self.scale_factor() as f32);

                            let webview_id = WebViewId::new();
                            let size = self.size();
                            let rect = DeviceRect::from_size(size);
                            let content_size =
                                self.get_content_size(rect, true, self.show_bookmark);
                            let size = content_size.size().to_f32() / hidpi_scale_factor;
                            let webview = WebView::new(
                                webview_id,
                                ViewportDetails {
                                    size,
                                    hidpi_scale_factor,
                                },
                            );

                            self.tab_manager.append_tab(webview, true);

                            let size = content_size.size().to_f32() / hidpi_scale_factor;
                            send_to_constellation(
                                &sender,
                                EmbedderToConstellationMessage::NewWebView(
                                    ServoUrl::parse("https://example.com").unwrap(),
                                    webview_id,
                                    ViewportDetails {
                                        size,
                                        hidpi_scale_factor,
                                    },
                                ),
                            );
                            let result = TabCreateResponse {
                                success: true,
                                id: webview_id,
                            };
                            let _ = response_sender.send(PromptResponse::Ok(result.to_json()));
                            return false;
                        } else if message.starts_with("OPEN_HISTORY_MENU") {
                            let request_str = message.strip_prefix("OPEN_HISTORY_MENU:").unwrap();
                            let request: OpenHistoryMenuRequest = serde_json::from_str(request_str)
                                .expect("Failed to parse OpenHistoryMenuRequest");
                            let _ = response_sender.send(PromptResponse::default());

                            if let Some(menu) = self.show_history_menu(&sender, request) {
                                self.webview_menu = Some(Box::new(menu));
                            }

                            return false;
                        } else if message == "UPDATE_BOOKMARK" {
                            self.show_bookmark = !bookmark_manager.bookmarks().is_empty();
                            compositor.resize(self.size(), self);
                            let _ = response_sender.send(PromptResponse::Ok(
                                serde_json::to_string(bookmark_manager.bookmarks()).unwrap(),
                            ));
                            return false;
                        } else if message == "BOOKMARK" {
                            if let Some(tab) = self.tab_manager.current_tab() {
                                if let Some(url) = tab.history().list.get(tab.history().current_idx)
                                {
                                    let url = url.to_string();
                                    // Ignore the bookmark if it starts with "verso://"
                                    if url.starts_with("verso://") {
                                        return false;
                                    }
                                    let bookmark_previously_shown =
                                        !bookmark_manager.bookmarks().is_empty();
                                    if let Some(_) = bookmark_manager
                                        .bookmarks()
                                        .iter()
                                        .position(|b| b.url == url)
                                    {
                                        let _ = bookmark_manager.remove_bookmark(
                                            BookmarkId::from_str(url.as_str()).unwrap(),
                                        );
                                    } else {
                                        bookmark_manager.append_bookmark(tab.title(), url);
                                    }
                                    let _ = response_sender.send(PromptResponse::Ok(
                                        serde_json::to_string(bookmark_manager.bookmarks())
                                            .unwrap(),
                                    ));

                                    self.show_bookmark = !bookmark_manager.bookmarks().is_empty();
                                    // We need to refresh the window if the need for bookmark to be displayed
                                    // has changed.
                                    if bookmark_previously_shown != self.show_bookmark {
                                        compositor.resize(self.size(), self);
                                    }
                                }
                            }
                            return false;
                        }

                        let _ = response_sender.send(PromptResponse::default());

                        /* Window */
                        match message.as_str() {
                            "NEW_WINDOW" => {
                                let _ = response_sender.send(PromptResponse::default());
                                return true;
                            }
                            "MINIMIZE" => {
                                self.window.set_minimized(true);
                                return false;
                            }
                            "MAXIMIZE" | "DBCLICK_PANEL" => {
                                let is_maximized = self.window.is_maximized();
                                self.window.set_maximized(!is_maximized);
                                return false;
                            }
                            "DRAG_WINDOW" => {
                                let _ = self.window.drag_window();
                                return false;
                            }
                            "DOWNLOAD" => {
                                self.create_tab(
                                    &sender,
                                    ServoUrl::parse("verso://resources/components/downloads.html")
                                        .unwrap(),
                                );
                            }
                            _ => {}
                        }

                        /* Main WebView */
                        if let Some(tab) = self.tab_manager.current_tab() {
                            let id = tab.id();
                            if message.starts_with("NAVIGATE_TO:") {
                                let unparsed_url = message.strip_prefix("NAVIGATE_TO:").unwrap();
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

                                let client = self.reqwest_client.clone();
                                let verso_internal_sender = self.verso_internal_sender.clone();
                                tokio::spawn(async move {
                                    let (should_download, resp) =
                                        check_should_download(&client, &url).await;
                                    if should_download && resp.is_some() {
                                        download_body(url, resp.unwrap(), verso_internal_sender)
                                            .await;
                                    } else {
                                        send_to_constellation(
                                            &sender,
                                            EmbedderToConstellationMessage::LoadUrl(
                                                id,
                                                ServoUrl::from_url(url.clone()),
                                            ),
                                        );
                                    }
                                });
                            } else if message == "OPEN_BOOKMARK_MANAGER" {
                                self.create_tab(
                                    &sender,
                                    ServoUrl::parse(
                                        "verso://resources/components/bookmark.html"
                                    )
                                    .unwrap(),
                                );
                            } else {
                                match message.as_str() {
                                    "PREV" => {
                                        send_to_constellation(
                                            &sender,
                                            EmbedderToConstellationMessage::TraverseHistory(
                                                id,
                                                TraversalDirection::Back(1),
                                            ),
                                        );
                                        // TODO Set EmbedderMsg::Status to None
                                    }
                                    "FORWARD" => {
                                        send_to_constellation(
                                            &sender,
                                            EmbedderToConstellationMessage::TraverseHistory(
                                                id,
                                                TraversalDirection::Forward(1),
                                            ),
                                        );
                                        // TODO Set EmbedderMsg::Status to None
                                    }
                                    "REFRESH" => {
                                        send_to_constellation(
                                            &sender,
                                            EmbedderToConstellationMessage::Reload(id),
                                        );
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
            EmbedderMsg::GetClipboardText(_webview_id, sender) => {
                let text = clipboard
                    .map(|c| {
                        c.get_text().unwrap_or_else(|e| {
                            log::warn!("Verso Panel failed to get clipboard text: {}", e);
                            String::new()
                        })
                    })
                    .unwrap_or_default();
                if let Err(e) = sender.send(Ok(text)) {
                    log::warn!("Verso Panel failed to send clipboard text: {}", e);
                }
            }
            EmbedderMsg::SetClipboardText(_webview_id, text) => {
                if let Some(c) = clipboard {
                    if let Err(e) = c.set_text(text) {
                        log::warn!("Verso Panel failed to set clipboard text: {}", e);
                    }
                }
            }
            EmbedderMsg::ShowContextMenu(_, servo_sender, _, _) => {
                #[cfg(linux)]
                if self.webview_menu.is_none() {
                    self.webview_menu =
                        Some(Box::new(self.show_context_menu(&sender, servo_sender)));
                } else {
                    let _ = servo_sender.send(ContextMenuResult::Ignored);
                }
                #[cfg(any(target_os = "windows", target_os = "macos"))]
                {
                    let context_menu = self.show_context_menu(servo_sender);
                    // FIXME: there's chance to lose the event since the channel is async.
                    if let Ok(event) = self.menu_event_receiver.try_recv() {
                        self.handle_context_menu_event(context_menu, &sender, event);
                    }
                }
            }
            e => {
                log::trace!("Verso Panel isn't supporting this message yet: {e:?}")
            }
        }
        false
    }

    /// Handle servo messages with webview menu. Return true it requests a new window.
    pub fn handle_servo_messages_with_webview_menu(
        &mut self,
        webview_id: WebViewId,
        message: EmbedderMsg,
        sender: &Sender<EmbedderToConstellationMessage>,
        _clipboard: Option<&mut Clipboard>,
        _compositor: &mut IOCompositor,
    ) -> bool {
        log::trace!("Verso WebView Menu {webview_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::WebViewBlurred => {
                self.focused_webview_id = None;
            }
            EmbedderMsg::WebViewFocused(webview_id) => {
                self.focused_webview_id = Some(webview_id);
            }
            EmbedderMsg::ShowSimpleDialog(_webview_id, simple_dialog) => match simple_dialog {
                SimpleDialog::Prompt {
                    message,
                    default: _,
                    response_sender,
                } => {
                    let _ = response_sender.send(PromptResponse::default());

                    #[cfg(linux)]
                    if message.starts_with("CONTEXT_MENU:") {
                        let json_str_msg = message.strip_prefix("CONTEXT_MENU:").unwrap();
                        let result =
                            serde_json::from_str::<ContextMenuUIResponse>(json_str_msg).unwrap();

                        self.handle_context_menu_event(sender, result);
                    }
                    if message.starts_with("HISTORY_MENU:") {
                        let json_str_msg = message.strip_prefix("HISTORY_MENU:").unwrap();
                        let result =
                            serde_json::from_str::<HistoryMenuUIResponse>(json_str_msg).unwrap();

                        self.handle_history_menu_event(sender, result);
                    }
                }
                _ => log::trace!("Verso context menu isn't supporting this prompt yet"),
            },
            #[cfg(linux)]
            EmbedderMsg::ShowContextMenu(_webview_id, servo_sender, _title, _options) => {
                if self.webview_menu.is_none() {
                    self.webview_menu =
                        Some(Box::new(self.show_context_menu(sender, servo_sender)));
                } else {
                    let _ = servo_sender.send(ContextMenuResult::Ignored);
                }
            }
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
        _sender: &Sender<EmbedderToConstellationMessage>,
        _clipboard: Option<&mut Clipboard>,
        _compositor: &mut IOCompositor,
    ) -> bool {
        log::trace!("Verso Prompt {webview_id:?} is handling Embedder message: {message:?}",);
        match message {
            EmbedderMsg::WebViewBlurred => {
                self.focused_webview_id = None;
            }
            EmbedderMsg::WebViewFocused(webview_id) => {
                self.focused_webview_id = Some(webview_id);
            }
            EmbedderMsg::ShowSimpleDialog(_webview_id, simple_dialog) => match simple_dialog {
                SimpleDialog::Alert {
                    message,
                    response_sender,
                } => {
                    let _ = response_sender.send(AlertResponse::default());

                    let Some(prompt) = self.tab_manager.prompt_by_prompt_id(webview_id) else {
                        log::error!("Prompt not found. WebView: {webview_id:?}");
                        return false;
                    };

                    let servo_sender = prompt.sender().unwrap();
                    match servo_sender {
                        PromptSender::AlertSender(sender) => {
                            let _ = sender.send(AlertResponse::default());
                        }
                        PromptSender::ConfirmSender(sender) => {
                            let result: ConfirmResponse = match message.as_str() {
                                "ok" => ConfirmResponse::Ok,
                                "cancel" => ConfirmResponse::Cancel,
                                _ => {
                                    log::error!("Invalid prompt action: {message}");
                                    ConfirmResponse::default()
                                }
                            };
                            let _ = sender.send(result);
                        }
                        PromptSender::InputSender(sender) => {
                            if let Ok(PromptInputResult { action, value }) =
                                serde_json::from_str::<PromptInputResult>(&message)
                            {
                                match action.as_str() {
                                    "ok" => {
                                        let _ = sender.send(PromptResponse::Ok(value));
                                    }
                                    "cancel" => {
                                        let _ = sender.send(PromptResponse::Cancel);
                                    }
                                    _ => {
                                        log::error!("Invalid prompt action: {message}");
                                        let _ = sender.send(PromptResponse::default());
                                    }
                                }
                            } else {
                                log::error!("Invalid prompt action: {message}");
                                let _ = sender.send(PromptResponse::default());
                            }
                        }
                        PromptSender::AllowDenySender(sender) => {
                            let result: AllowOrDeny = match message.as_str() {
                                "allow" => AllowOrDeny::Allow,
                                "deny" => AllowOrDeny::Deny,
                                _ => {
                                    log::error!("Invalid prompt action: {message}");
                                    AllowOrDeny::Deny
                                }
                            };
                            let _ = sender.send(result);
                        }
                        PromptSender::HttpBasicAuthSender(sender) => {
                            if let Ok(HttpBasicAuthInputResult { action, auth }) =
                                serde_json::from_str::<HttpBasicAuthInputResult>(&message)
                            {
                                match action.as_str() {
                                    "signin" => {
                                        let _ = sender.send(Some(auth));
                                    }
                                    "cancel" => {
                                        let _ = sender.send(None);
                                    }
                                    _ => {
                                        let _ = sender.send(None);
                                    }
                                };
                            } else {
                                log::error!("Invalid prompt action: {message}");
                                let _ = sender.send(None);
                            }
                        }
                    }
                }
                _ => {
                    log::trace!("Unsupported prompt type");
                }
            },
            EmbedderMsg::ShowContextMenu(_, sender, _, _) => {
                let _ = sender.send(ContextMenuResult::Ignored);
            }
            e => {
                log::trace!("Verso Dialog isn't supporting this message yet: {e:?}")
            }
        }
        false
    }
    /// Handle Verso internal messages with webview.
    pub fn handle_verso_internal_messages_with_webview(
        &self,
        message: &str,
        response_sender: IpcSender<PromptResponse>,
        sender: &Sender<EmbedderToConstellationMessage>,
        tab: &Tab,
    ) {
        // If the prompt is come from Verso's Downloads or Bookmark page, update the status
        if message == "DOWNLOAD_STATUS_GET" {
            let _ = self
                .verso_internal_sender
                .send(VersoInternalMsg::UpdateDownloadsPage(response_sender));
            return;
        } else if message.starts_with("ABORT_DOWNLOAD::") {
            if let Some(id) = message.strip_prefix("ABORT_DOWNLOAD::") {
                let _ = response_sender.send(PromptResponse::Cancel);
                let _ = self
                    .verso_internal_sender
                    .send(VersoInternalMsg::AbortDownload(
                        DownloadId::from_str(id).unwrap(),
                    ));
            }
            return;
        } else if message.starts_with("BOOKMARK_LIST_GET") {
            let _ = self
                .verso_internal_sender
                .send(VersoInternalMsg::UpdateBookmarkManager(response_sender));
            return;
        } else if message.starts_with("BOOKMARK_RENAME::") {
            if let Some(id) = message.strip_prefix("BOOKMARK_RENAME::") {
                let params: Vec<&str> = id.split("::").collect();
                if params.len() != 2 {
                    log::error!("Invalid parameters for BOOKMARK_RENAME");
                    return;
                }
                let _ = response_sender.send(PromptResponse::Cancel);
                let _ = self.verso_internal_sender.send(
                    if let Ok(id) = BookmarkId::from_str(params[0]) {
                        VersoInternalMsg::BookmarkRename(id, params[1].to_string())
                    } else {
                        log::error!("Invalid bookmark ID: {}", id);
                        return;
                    },
                );
            }
            return;
        } else if message.starts_with("BOOKMARK_REMOVE::") {
            if let Some(id) = message.strip_prefix("BOOKMARK_REMOVE::") {
                let _ = response_sender.send(PromptResponse::Cancel);
                let _ = self
                    .verso_internal_sender
                    .send(if let Ok(id) = BookmarkId::from_str(id) {
                        VersoInternalMsg::BookmarkRemove(id)
                    } else {
                        log::error!("Invalid bookmark ID: {}", id);
                        return;
                    });
            }
            return;
        } else if message.starts_with("NAVIGATE_TO::") {
            let url = message.strip_prefix("NAVIGATE_TO::").unwrap();
            let url = match Url::parse(url) {
                Ok(url_parsed) => url_parsed,
                Err(e) => {
                    if e == url::ParseError::RelativeUrlWithoutBase {
                        Url::parse(&format!("https://{}", url)).unwrap()
                    } else {
                        panic!("Verso Panel failed to parse URL: {}", e);
                    }
                }
            };
            send_to_constellation(
                &sender,
                EmbedderToConstellationMessage::LoadUrl(tab.id(), ServoUrl::from_url(url.clone())),
            );
            return;
        }
    }
}

/// Blocking execute a script on this webview
pub fn execute_script(
    constellation_sender: &Sender<EmbedderToConstellationMessage>,
    webview: &WebViewId,
    js: impl ToString,
) -> WebDriverJSResult {
    let (result_sender, result_receiver) = ipc::channel::<WebDriverJSResult>().unwrap();
    send_to_constellation(
        constellation_sender,
        EmbedderToConstellationMessage::WebDriverCommand(WebDriverCommandMsg::ScriptCommand(
            webview.0,
            WebDriverScriptCommand::ExecuteScript(js.to_string(), result_sender),
        )),
    );
    result_receiver.recv().unwrap()
}
