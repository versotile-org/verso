use base::id::WebViewId;
use constellation_traits::EmbedderToConstellationMessage;
use crossbeam_channel::Sender;
use embedder_traits::{
    AlertResponse, AllowOrDeny, AuthenticationResponse, ConfirmResponse, PromptResponse,
    ViewportDetails,
};
use euclid::Scale;
use ipc_channel::ipc::IpcSender;
use serde::{Deserialize, Serialize};
use servo_url::ServoUrl;
use webrender_api::units::DeviceRect;

use crate::{verso::send_to_constellation, webview::WebView};

/// Prompt Type
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum PromptType {
    /// Alert dialog
    ///
    /// <https://developer.mozilla.org/en-US/docs/Web/API/Window/alert>
    Alert(String),
    /// Confitm dialog, Ok/Cancel
    ///
    /// <https://developer.mozilla.org/en-US/docs/Web/API/Window/confirm>
    OkCancel(String),
    /// Confirm dialog, Allow/Deny
    ///
    /// <https://developer.mozilla.org/en-US/docs/Web/API/Window/confirm>
    AllowDeny(String),
    /// Input dialog
    ///
    /// <https://developer.mozilla.org/en-US/docs/Web/API/Window/prompt>
    Input(String, Option<String>),
    /// HTTP basic authentication dialog (username / password)
    ///
    /// <https://developer.mozilla.org/en-US/docs/Web/HTTP/Authentication#basic>
    HttpBasicAuth,
}

/// Prompt Sender, used to send prompt result back to the caller
#[derive(Clone)]
pub enum PromptSender {
    /// Alert sender
    AlertSender(IpcSender<AlertResponse>),
    /// Ok/Cancel, Yes/No sender
    ConfirmSender(IpcSender<ConfirmResponse>),
    /// Input sender
    InputSender(IpcSender<PromptResponse>),
    /// Allow/Deny Permission sender
    AllowDenySender(IpcSender<AllowOrDeny>),
    /// HTTP basic authentication sender
    HttpBasicAuthSender(IpcSender<Option<AuthenticationResponse>>),
}

/// Prompt input result send from prompt dialog to backend
/// - action: "ok" / "cancel"
/// - value: user input value in input prompt
///
/// Behavior:
/// - **Ok**: return string, or an empty string if user leave input empty
/// - **Cancel**: return null
///
/// <https://developer.mozilla.org/en-US/docs/Web/API/Window/prompt#return_value>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptInputResult {
    /// User action: "ok" / "cancel"
    pub action: String,
    /// User input value
    pub value: String,
}

/// Prompt input result send from prompt dialog to backend
/// - action: "signin" / "cancel"
/// - auth: { username: string, password: string }
///
/// Behavior:
/// - **signin**: return { username: string, password: string }
/// - **cancel**: return { username: null, password: null }
#[derive(Debug, Serialize, Deserialize)]
pub struct HttpBasicAuthInputResult {
    /// User action: "signin" / "cancel"
    pub action: String,
    /// User input value
    pub auth: AuthenticationResponse,
}

/// Prompt Dialog
#[derive(Clone)]
pub struct PromptDialog {
    webview: WebView,
    prompt_sender: Option<PromptSender>,
}

impl PromptDialog {
    /// New prompt dialog
    pub fn new() -> Self {
        PromptDialog {
            webview: WebView::new(WebViewId::new(), ViewportDetails::default()),
            prompt_sender: None,
        }
    }
    /// Get prompt webview
    pub fn webview(&self) -> &WebView {
        &self.webview
    }
    /// Get prompt webview ID
    pub fn id(&self) -> WebViewId {
        self.webview.webview_id
    }

    /// Get prompt sender. Send user interaction result back to caller.
    pub fn sender(&self) -> Option<PromptSender> {
        self.prompt_sender.clone()
    }

    /// Resize prompt webview size with new window context size
    ///
    /// ## Example:
    /// ```rust
    /// let rect = window.webview.as_ref().unwrap().rect;
    /// let content_size = window.get_content_size(rect);
    /// prompt.set_size(content_size);
    /// ```
    pub fn set_size(&mut self, rect: DeviceRect) {
        self.webview.set_size(rect);
    }

    /// Show alert prompt.
    ///
    /// After you call `alert(..)`, you must call `sender()` to get prompt sender,
    /// then send user interaction result back to caller.
    ///
    /// ## Example
    ///
    /// ```rust
    /// if let Some(PromptSender::AlertSender(sender)) = prompt.sender() {
    ///     let _ = sender.send(());
    /// }
    /// ```
    pub fn alert(
        &mut self,
        sender: &Sender<EmbedderToConstellationMessage>,
        rect: DeviceRect,
        scale_factor: f32,
        message: String,
        prompt_sender: IpcSender<AlertResponse>,
    ) {
        self.prompt_sender = Some(PromptSender::AlertSender(prompt_sender));
        self.show(sender, rect, scale_factor, PromptType::Alert(message));
    }

    /// Show Ok/Cancel confirm prompt
    ///
    /// After you call `ok_cancel(..)`, you must call `sender()` to get prompt sender,
    /// then send user interaction result back to caller.
    ///
    /// ## Example
    ///
    /// ```rust
    /// if let Some(PromptSender::ConfirmSender(sender)) = prompt.sender() {
    ///     let _ = sender.send(PromptResult::Primary);
    /// }
    /// ```
    pub fn ok_cancel(
        &mut self,
        sender: &Sender<EmbedderToConstellationMessage>,
        rect: DeviceRect,
        scale_factor: f32,
        message: String,
        prompt_sender: IpcSender<ConfirmResponse>,
    ) {
        self.prompt_sender = Some(PromptSender::ConfirmSender(prompt_sender));
        self.show(sender, rect, scale_factor, PromptType::OkCancel(message));
    }

    /// Show Yes/No confirm prompt
    ///
    /// After you call `allow_deny(..)`, you must call `sender()` to get prompt sender,
    /// then send user interaction result back to caller.
    ///
    /// ## Example
    ///
    /// ```rust
    /// let mut prompt = PromptDialog::new();
    /// prompt.allow_deny(sender, rect, message, prompt_sender);
    /// if let Some(PromptSender::AllowDenySender(sender)) = prompt.sender() {
    ///     let _ = sender.send(AllowOrDeny::Allow);
    /// }
    /// ```
    pub fn allow_deny(
        &mut self,
        sender: &Sender<EmbedderToConstellationMessage>,
        rect: DeviceRect,
        scale_factor: f32,
        message: String,
        prompt_sender: PromptSender,
    ) {
        self.prompt_sender = Some(prompt_sender);
        self.show(sender, rect, scale_factor, PromptType::AllowDeny(message));
    }

    /// Show input prompt
    ///
    /// After you call `input(..)`, you must call `sender()` to get prompt sender,
    /// then send user interaction result back to caller.
    ///
    /// ## Example
    ///
    /// ```rust
    /// if let Some(PromptSender::InputSender(sender)) = prompt.sender() {
    ///     let _ = sender.send(Some("user input value".to_string()));
    /// }
    /// ```
    pub fn input(
        &mut self,
        sender: &Sender<EmbedderToConstellationMessage>,
        rect: DeviceRect,
        scale_factor: f32,
        message: String,
        default_value: Option<String>,
        prompt_sender: IpcSender<PromptResponse>,
    ) {
        self.prompt_sender = Some(PromptSender::InputSender(prompt_sender));
        self.show(
            sender,
            rect,
            scale_factor,
            PromptType::Input(message, default_value),
        );
    }

    /// Show input prompt
    ///
    /// After you call `input(..)`, you must call `sender()` to get prompt sender,
    /// then send user interaction result back to caller.
    ///
    /// ## Example
    ///
    /// ```rust
    /// if let Some(PromptSender::HttpBasicAuthSender(sender)) = prompt.sender() {
    ///     let _ = sender.send(AuthenticationResponse {
    ///         username: "user".to_string(),
    ///         password: "password".to_string(),
    ///     });
    /// }
    /// ```
    pub fn http_basic_auth(
        &mut self,
        sender: &Sender<EmbedderToConstellationMessage>,
        rect: DeviceRect,
        scale_factor: f32,
        prompt_sender: IpcSender<Option<AuthenticationResponse>>,
    ) {
        self.prompt_sender = Some(PromptSender::HttpBasicAuthSender(prompt_sender));
        self.show(sender, rect, scale_factor, PromptType::HttpBasicAuth);
    }

    fn show(
        &mut self,
        sender: &Sender<EmbedderToConstellationMessage>,
        rect: DeviceRect,
        scale_factor: f32,
        prompt_type: PromptType,
    ) {
        self.webview.set_size(rect);

        let hidpi_scale_factor = Scale::new(scale_factor);
        let size = rect.size().to_f32() / hidpi_scale_factor;
        send_to_constellation(
            sender,
            EmbedderToConstellationMessage::NewWebView(
                self.resource_url(prompt_type),
                self.webview.webview_id,
                ViewportDetails {
                    size,
                    hidpi_scale_factor,
                },
            ),
        );
    }

    fn resource_url(&self, prompt_type: PromptType) -> ServoUrl {
        let url = match prompt_type {
            PromptType::Alert(msg) => {
                format!("verso://resources/components/prompt/alert.html?msg={msg}")
            }
            PromptType::OkCancel(msg) => {
                format!("verso://resources/components/prompt/ok_cancel.html?msg={msg}")
            }
            PromptType::AllowDeny(msg) => {
                format!("verso://resources/components/prompt/allow_deny.html?msg={msg}")
            }
            PromptType::Input(msg, default_value) => {
                let mut url = format!("verso://resources/components/prompt/prompt.html?msg={msg}");
                if let Some(default_value) = default_value {
                    url.push_str(&format!("&defaultValue={}", default_value));
                }
                url
            }
            PromptType::HttpBasicAuth => {
                "verso://resources/components/prompt/http_basic_auth.html".to_string()
            }
        };
        ServoUrl::parse(&url).unwrap()
    }
}

impl Default for PromptDialog {
    fn default() -> Self {
        Self::new()
    }
}
