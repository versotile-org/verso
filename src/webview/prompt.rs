use base::id::WebViewId;
use compositing_traits::ConstellationMsg;
use crossbeam_channel::Sender;
use embedder_traits::{PermissionRequest, PromptResult};
use ipc_channel::ipc::IpcSender;
use serde::{Deserialize, Serialize};
use servo_url::ServoUrl;
use webrender_api::units::DeviceIntRect;

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
    /// Confirm dialog, Yes/No
    ///
    /// <https://developer.mozilla.org/en-US/docs/Web/API/Window/confirm>
    YesNo(String),
    /// Input dialog
    ///
    /// <https://developer.mozilla.org/en-US/docs/Web/API/Window/prompt>
    Input(String, Option<String>),
}

/// Prompt Sender, used to send prompt result back to the caller
#[derive(Clone)]
pub enum PromptSender {
    /// Alert sender
    AlertSender(IpcSender<()>),
    /// Ok/Cancel, Yes/No sender
    ConfirmSender(IpcSender<PromptResult>),
    /// Input sender
    InputSender(IpcSender<Option<String>>),
    /// Yes/No Permission sender
    PermissionSender(IpcSender<PermissionRequest>),
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
            webview: WebView::new(WebViewId::new(), DeviceIntRect::zero()),
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
    pub fn set_size(&mut self, rect: DeviceIntRect) {
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
        sender: &Sender<ConstellationMsg>,
        rect: DeviceIntRect,
        message: String,
        prompt_sender: IpcSender<()>,
    ) {
        self.prompt_sender = Some(PromptSender::AlertSender(prompt_sender));
        self.show(sender, rect, PromptType::Alert(message));
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
        sender: &Sender<ConstellationMsg>,
        rect: DeviceIntRect,
        message: String,
        prompt_sender: IpcSender<PromptResult>,
    ) {
        self.prompt_sender = Some(PromptSender::ConfirmSender(prompt_sender));
        self.show(sender, rect, PromptType::OkCancel(message));
    }

    /// Show Yes/No confirm prompt
    ///
    /// After you call `yes_no(..)`, you must call `sender()` to get prompt sender,
    /// then send user interaction result back to caller.
    ///
    /// ## Example
    ///
    /// ```rust
    /// let mut prompt = PromptDialog::new();
    /// prompt.yes_no(sender, rect, message, prompt_sender);
    /// if let Some(PromptSender::PermissionSender(sender)) = prompt.sender() {
    ///     let _ = sender.send(PermissionRequest::Granted);
    /// }
    /// ```
    pub fn yes_no(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        rect: DeviceIntRect,
        message: String,
        prompt_sender: PromptSender,
    ) {
        self.prompt_sender = Some(prompt_sender);
        self.show(sender, rect, PromptType::YesNo(message));
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
        sender: &Sender<ConstellationMsg>,
        rect: DeviceIntRect,
        message: String,
        default_value: Option<String>,
        prompt_sender: IpcSender<Option<String>>,
    ) {
        self.prompt_sender = Some(PromptSender::InputSender(prompt_sender));
        self.show(sender, rect, PromptType::Input(message, default_value));
    }

    fn show(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        rect: DeviceIntRect,
        prompt_type: PromptType,
    ) {
        self.webview.set_size(rect);
        send_to_constellation(
            sender,
            ConstellationMsg::NewWebView(self.resource_url(prompt_type), self.webview.webview_id),
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
            PromptType::YesNo(msg) => {
                format!("verso://resources/components/prompt/yes_no.html?msg={msg}")
            }
            PromptType::Input(msg, default_value) => {
                let mut url = format!("verso://resources/components/prompt/prompt.html?msg={msg}");
                if let Some(default_value) = default_value {
                    url.push_str(&format!("&defaultValue={}", default_value));
                }
                url
            }
        };
        ServoUrl::parse(&url).unwrap()
    }
}
