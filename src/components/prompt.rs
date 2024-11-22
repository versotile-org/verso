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
    /// Alert
    Alert(String),
    /// Confirm, Cancel / Ok
    OkCancel(String),
    /// Confirm, No, Yes
    YesNo(String),
    /// Input, message and default value
    Input(String, Option<String>),
}

/// Prompt Sender, used to send prompt result back to the caller
#[derive(Clone)]
pub enum PromptSender {
    /// Ok/Cancel, Yes/No sender
    ConfirmSender(IpcSender<PromptResult>),
    /// Input sender
    InputSender(IpcSender<Option<String>>),
}

/// Input prompt result from prompt dialog
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputPromptResult {
    /// User action: ok / cancel
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
        window: &mut Window,
        message: String,
    ) {
        self.show(sender, window, PromptType::Alert(message));
    }

    /// show alert dialog on a window
    pub fn ok_cancel(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        window: &mut Window,
        message: String,
        prompt_sender: IpcSender<PromptResult>,
    ) {
        self.prompt_sender = Some(PromptSender::ConfirmSender(prompt_sender));
        self.show(sender, window, PromptType::OkCancel(message));
    }

    /// show alert dialog on a window
    pub fn yes_no(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        window: &mut Window,
        message: String,
        prompt_sender: IpcSender<PromptResult>,
    ) {
        self.prompt_sender = Some(PromptSender::ConfirmSender(prompt_sender));
        self.show(sender, window, PromptType::YesNo(message));
    }

    /// show alert dialog on a window
    pub fn input(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        window: &mut Window,
        message: String,
        default_value: Option<String>,
        prompt_sender: IpcSender<Option<String>>,
    ) {
        self.prompt_sender = Some(PromptSender::InputSender(prompt_sender));
        self.show(sender, window, PromptType::Input(message, default_value));
    }

    /// show prompt dialog on a window
    fn show(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        window: &mut Window,
        prompt_type: PromptType,
    ) {
        let rect = window.webview.as_ref().unwrap().rect.clone();
        self.webview.set_size(rect);

        send_to_constellation(
            sender,
            ConstellationMsg::NewWebView(self.resource_url(prompt_type), self.webview.webview_id),
        );
        window.append_dialog_webview(self.webview().clone());
    }

    fn resource_url(&self, prompt_type: PromptType) -> ServoUrl {
        let url = match prompt_type {
            PromptType::Alert(msg) => {
                // TODO: sanitize message
                format!("verso://alert.html?msg={msg}")
            }
            PromptType::OkCancel(msg) => {
                // TODO: sanitize message
                format!("verso://ok_cancel.html?msg={msg}")
            }
            PromptType::YesNo(msg) => {
                // TODO: sanitize message
                format!("verso://ok_cancel.html?msg={msg}")
            }
            PromptType::Input(msg, default_value) => {
                // TODO: sanitize message
                let mut url = format!("verso://prompt.html?msg={msg}");
                if let Some(default_value) = default_value {
                    url.push_str(&format!("&defaultValue={}", default_value));
                }
                url
            }
        };
        ServoUrl::parse(&url).unwrap()
    }
}
