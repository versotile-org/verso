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
enum PromptType {
    /// Alert
    Alert(String),
    /// Message + OK button
    Confirm,
    /// Input included
    Prompt,
}

/// Prompt Dialog
#[derive(Debug, Clone)]
pub struct PromptDialog {
    webview: WebView,
}

/// PromptDialogAttributes
#[derive(Debug, Clone)]
pub struct PromptDialogAttributes {
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
            webview: WebView::new(WebViewId::new(), rect),
        }
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

    /// show alert dialog on a window
    pub fn alert(&mut self, sender: &Sender<ConstellationMsg>, window: &mut Window, message: &str) {
        self.show(sender, window, PromptType::Alert(message.to_string()), None);
    }

    /// show prompt dialog on a window
    fn show(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        window: &mut Window,
        prompt_type: PromptType,
        _position: Option<PhysicalPosition<f64>>,
    ) {
        let scale_factor = window.scale_factor();
        // self.set_position(window, position, scale_factor);
        let x = PromptDialog::calc_dialog_start_x(window, (500.0 * scale_factor) as i32);
        let origin = DeviceIntPoint::new(x, (72.0 * scale_factor) as i32);
        let size = Size2D::new((500.0 * scale_factor) as i32, (150.0 * scale_factor) as i32);
        let rect = DeviceIntRect::from_origin_and_size(origin, size);
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
            _ => format!("verso://alert.html"),
        };
        ServoUrl::parse(&url).unwrap()
    }

    fn calc_dialog_start_x(verso_window: &Window, logical_width: i32) -> i32 {
        let size = verso_window.window.inner_size();
        std::cmp::max(0, (size.width as i32 - logical_width) / 2)
    }
}
