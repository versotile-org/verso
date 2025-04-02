use embedder_traits::ContextMenuResult;
use ipc_channel::ipc::IpcSender;
/* macOS, Windows Native Implementation */
#[cfg(any(target_os = "macos", target_os = "windows"))]
use muda::{ContextMenu as MudaContextMenu, Menu as MudaMenu};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/* Wayland Implementation */
#[cfg(linux)]
use crate::{verso::send_to_constellation, webview::WebView, window::Window};
#[cfg(linux)]
use base::id::WebViewId;
#[cfg(linux)]
use constellation_traits::ConstellationMsg;
#[cfg(linux)]
use crossbeam_channel::Sender;
#[cfg(linux)]
use serde::{Deserialize, Serialize};
#[cfg(linux)]
use servo_url::ServoUrl;
#[cfg(linux)]
use webrender_api::units::DeviceIntRect;
#[cfg(linux)]
use winit::dpi::{LogicalPosition, PhysicalPosition};

/// Basic menu type building block
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct Menu(pub MudaMenu);
/// Basic menu type building block
#[cfg(linux)]
#[derive(Debug, Clone)]
pub struct Menu(pub Vec<MenuItem>);

/// The Context Menu of the Window. It will be opened when users right click on any window's
/// webview.
///
/// **Platform Specific**
/// - macOS / Windows: This will be native context menu supported by each OS.
/// - Wayland: Winit doesn't support popup surface of Wayland at the moment. So we utilize a custom
///   webview implementation.
#[derive(Clone)]
pub struct ContextMenu {
    /// IpcSender to send the context menu result to the Servo
    servo_result_sender: Option<IpcSender<ContextMenuResult>>, // None if sender already sent
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    menu: MudaMenu,
    #[cfg(linux)]
    menu_items: Vec<MenuItem>,
    /// The webview that the context menu is attached to
    #[cfg(linux)]
    pub(crate) webview: WebView,
    /// Menu position, used for positioning the context menu by CSS
    #[cfg(linux)]
    position: LogicalPosition<f64>,
}

impl ContextMenu {
    /// Create context menu with custom items
    ///
    /// **Platform Specific**
    /// - macOS / Windows: Creates a context menu by muda crate with natvie OS support
    /// - Wayland: Creates a context menu with webview implementation
    pub fn new_with_menu(servo_result_sender: IpcSender<ContextMenuResult>, menu: Menu) -> Self {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            Self {
                servo_result_sender: Some(servo_result_sender),
                menu: menu.0,
            }
        }
        #[cfg(linux)]
        {
            let webview_id = WebViewId::new();
            let webview = WebView::new(webview_id, DeviceIntRect::zero());

            Self {
                servo_result_sender: Some(servo_result_sender),
                menu_items: menu.0,
                webview,
                position: LogicalPosition::new(0.0, 0.0),
            }
        }
    }

    /// Send the context menu result back to the Servo. Can only be sent once.
    pub fn send_result_to_servo(&mut self, result: ContextMenuResult) {
        if let Some(sender) = self.servo_result_sender.take() {
            let _ = sender.send(result);
        }
    }
}

impl Drop for ContextMenu {
    fn drop(&mut self) {
        self.send_result_to_servo(ContextMenuResult::Dismissed);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ContextMenu {
    /// Show the context menu on current cursor position
    ///
    /// This function returns when the context menu is dismissed
    pub fn show(&self, rwh: impl HasWindowHandle) {
        // Show the context menu
        unsafe {
            let wh = rwh.window_handle().unwrap();
            match wh.as_raw() {
                #[cfg(target_os = "macos")]
                RawWindowHandle::AppKit(handle) => {
                    // use objc2
                    assert!(
                        objc2_foundation::is_main_thread(),
                        "can only access AppKit handles on the main thread"
                    );
                    let ns_view = handle.ns_view.as_ptr();
                    self.menu.show_context_menu_for_nsview(ns_view, None);
                }
                #[cfg(target_os = "windows")]
                RawWindowHandle::Win32(handle) => {
                    let hwnd = handle.hwnd;
                    self.menu.show_context_menu_for_hwnd(hwnd.into(), None);
                }
                handle => unreachable!("unknown handle {handle:?} for platform"),
            }
        }
    }
}

#[cfg(linux)]
impl ContextMenu {
    /// Show the context menu to current cursor position
    pub fn show(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        window: &mut Window,
        position: PhysicalPosition<f64>,
    ) {
        self.position = position.to_logical(window.scale_factor());
        self.webview.rect = DeviceIntRect::from_size(window.outer_size());

        send_to_constellation(
            sender,
            ConstellationMsg::NewWebView(self.resource_url(), self.webview.webview_id),
        );
    }

    /// Get webview of the context menu
    pub fn webview(&self) -> &WebView {
        &self.webview
    }

    /// Get resource URL of the context menu
    fn resource_url(&self) -> ServoUrl {
        let items_json: String = self.to_items_json();
        let url_str = format!(
            "verso://resources/components/context_menu.html?items={}&pos_x={}&pos_y={}",
            items_json, self.position.x, self.position.y
        );
        ServoUrl::parse(&url_str).unwrap()
    }

    /// get item json
    fn to_items_json(&self) -> String {
        serde_json::to_string(&self.menu_items).unwrap()
    }
}

/// Menu Item
#[cfg(linux)]
#[derive(Debug, Clone, Serialize)]
pub struct MenuItem {
    id: String,
    /// label of the menu item
    pub label: String,
    /// Whether the menu item is enabled
    pub enabled: bool,
}

#[cfg(linux)]
impl MenuItem {
    /// Create a new menu item
    pub fn new(id: Option<&str>, label: &str, enabled: bool) -> Self {
        let id = id.unwrap_or(label);
        Self {
            id: id.to_string(),
            label: label.to_string(),
            enabled,
        }
    }
    /// Get the id of the menu item
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Context Menu Click Result
#[cfg(linux)]
#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct ContextMenuUIResponse {
    /// The id of the menu item
    pub id: Option<String>,
    /// Close the context menu
    pub close: bool,
}
