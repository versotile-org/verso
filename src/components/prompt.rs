use base::id::WebViewId;
use compositing_traits::ConstellationMsg;
use crossbeam_channel::Sender;
use euclid::Size2D;
use servo_url::ServoUrl;
use webrender_api::units::{DeviceIntPoint, DeviceIntRect, DeviceIntSize};
use winit::dpi::PhysicalPosition;

use crate::{verso::send_to_constellation, webview::WebView, window::Window};

/// Prompt Type
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PromptType {
    /// Alert
    Alert,
    /// Message + OK button
    Confirm,
    /// Input included
    Prompt,
}

/// Prompt Dialog
#[derive(Debug, Clone)]
pub struct PromptDialog {
    prompt_type: PromptType,
    webview: WebView,
}

/// PromptDialogAttributes
#[derive(Debug, Clone)]
pub struct PromptDialogAttributes {
    prompt_type: PromptType,
    size: DeviceIntSize,
    position: DeviceIntPoint,
}

/// Prompt Dialog Builder
#[derive(Debug, Clone)]
pub struct PromptDialogBuilder {
    attributes: PromptDialogAttributes,
}

impl PromptDialogBuilder {
    /// new builder
    pub fn new() -> Self {
        Self {
            attributes: PromptDialogAttributes {
                prompt_type: PromptType::Alert,
                size: DeviceIntSize::zero(),
                position: DeviceIntPoint::zero(),
            },
        }
    }

    /// build prompt
    pub fn build(&self) -> PromptDialog {
        let rect =
            DeviceIntRect::from_origin_and_size(self.attributes.position, self.attributes.size);

        PromptDialog {
            prompt_type: self.attributes.prompt_type.clone(),
            webview: WebView::new(WebViewId::new(), rect),
        }
    }

    /// set prompt type
    pub fn with_prompt_type(mut self, prompt_type: PromptType) -> Self {
        self.attributes.prompt_type = prompt_type;
        self
    }

    /// set prompt width and height
    pub fn with_size(mut self, size: DeviceIntSize) -> Self {
        self.attributes.size = size;
        self
    }
}

impl PromptDialog {
    /// get prompt webview
    pub fn webview(&self) -> &WebView {
        &self.webview
    }

    /// show prompt dialog on a window
    pub fn show(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        window: &mut Window,
        _position: Option<PhysicalPosition<f64>>,
    ) {
        // let scale_factor = window.scale_factor();
        // self.set_position(window, position, scale_factor);
        let x = PromptDialog::calc_dialog_start_x(window);
        let origin = DeviceIntPoint::new(x, 72);
        let size = Size2D::new(1000, 300);
        let rect = DeviceIntRect::from_origin_and_size(origin, size);
        self.webview.set_size(rect);

        send_to_constellation(
            sender,
            ConstellationMsg::NewWebView(self.resource_url(), self.webview.webview_id),
        );
        window.append_dialog_webview(self.webview().clone());
    }

    fn resource_url(&self) -> ServoUrl {
        let url = match self.prompt_type {
            _ => format!("verso://prompt.html"),
        };
        ServoUrl::parse(&url).unwrap()
    }

    fn calc_dialog_start_x(verso_window: &Window) -> i32 {
        let size = verso_window.window.inner_size();
        std::cmp::max(0, (size.width as i32 - 1000) / 2)
    }
}
