use base::id::WebViewId;
use compositing_traits::ConstellationMsg;
use crossbeam_channel::Sender;
use embedder_traits::PromptResult;
use ipc_channel::ipc::IpcSender;
use serde::{Deserialize, Serialize};
use servo_url::ServoUrl;
use webrender_api::units::DeviceIntRect;

use crate::{verso::send_to_constellation, webview::WebView, window::Window};

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
}

/// Prompt input result send from prompt dialog to backend
///
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
    /// new prompt dialog
    pub fn new() -> Self {
        PromptDialog {
            webview: WebView::new(WebViewId::new(), DeviceIntRect::zero()),
            prompt_sender: None,
        }
    }
    /// get prompt webview
    pub fn webview(&self) -> &WebView {
        &self.webview
    }

    /// get prompt sender
    pub fn sender(&self) -> Option<PromptSender> {
        self.prompt_sender.clone()
    }

    /// show alert dialog on a window
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

    /// show alert dialog on a window
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

    /// show alert dialog on a window
    pub fn yes_no(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        rect: DeviceIntRect,
        message: String,
        prompt_sender: IpcSender<PromptResult>,
    ) {
        self.prompt_sender = Some(PromptSender::ConfirmSender(prompt_sender));
        self.show(sender, rect, PromptType::YesNo(message));
    }

    /// show alert dialog on a window
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

    /// show prompt dialog on a window
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
                // TODO: sanitize message
                format!("verso://resources/components/prompt/alert.html?msg={msg}")
            }
            PromptType::OkCancel(msg) => {
                // TODO: sanitize message
                format!("verso://resources/components/prompt/ok_cancel.html?msg={msg}")
            }
            PromptType::YesNo(msg) => {
                // TODO: sanitize message
                format!("verso://resources/components/prompt/yes_no.html?msg={msg}")
            }
            PromptType::Input(msg, default_value) => {
                // TODO: sanitize message
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
