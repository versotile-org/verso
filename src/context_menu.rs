use base::id::WebViewId;
use euclid::{Point2D, Size2D};

/* macOS, Windows Native Implementation */
#[cfg(any(target_os = "macos", target_os = "windows"))]
use muda::{ContextMenu as MudaContextMenu, Menu};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/* Wayland Implementation */
#[cfg(linux)]
use crate::{verso::send_to_constellation, webview::WebView, window::Window};
#[cfg(linux)]
use compositing_traits::ConstellationMsg;
#[cfg(linux)]
use crossbeam_channel::Sender;
#[cfg(linux)]
use serde::{Deserialize, Serialize};
#[cfg(linux)]
use servo_url::ServoUrl;
#[cfg(linux)]
use webrender_api::units::DeviceIntRect;
#[cfg(linux)]
use winit::dpi::PhysicalPosition;

/// Context Menu
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct ContextMenu {
    menu: Menu,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ContextMenu {
    /// Create context menu with custom items
    pub fn new_with_menu(menu: Menu) -> Self {
        Self { menu }
    }

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

/// Context Menu
#[cfg(linux)]
#[derive(Debug, Clone)]
pub struct ContextMenu {
    menu_items: Vec<MenuItem>,
    /// The webview that the context menu is attached to
    webview: WebView,
}

#[cfg(linux)]
impl ContextMenu {
    /// Create a dialog in the window.
    ///
    /// Often used by calling window.alert() or window.confirm() in the web page.
    pub fn new_with_menu(menu_items: Vec<MenuItem>) -> Self {
        let webview_id = WebViewId::new();
        let webview = WebView::new(webview_id, DeviceIntRect::zero());

        Self {
            menu_items,
            webview,
        }
    }

    /// Show the context menu to current cursor position
    pub fn show(
        &mut self,
        sender: &Sender<ConstellationMsg>,
        window: &mut Window,
        position: PhysicalPosition<f64>,
    ) {
        let scale_factor = window.scale_factor();
        self.set_position(position, scale_factor);

        send_to_constellation(
            sender,
            ConstellationMsg::NewWebView(self.resource_url(), self.webview.webview_id),
        );
        window.append_dialog_webview(self.webview.clone());
    }

    /// Get webview of the context menu
    pub fn webview(&self) -> &WebView {
        &self.webview
    }

    /// Get resource URL of the context menu
    fn resource_url(&self) -> ServoUrl {
        let items_json: String = self.to_items_json();
        let url_str = format!("verso://context_menu.html?items={}", items_json);
        ServoUrl::parse(&url_str).unwrap()
    }

    /// Set the position of the context menu
    fn set_position(&mut self, position: PhysicalPosition<f64>, scale_factor: f64) {
        // Translate position to origin
        let origin = Point2D::new(position.x as i32, position.y as i32);

        // Calculate menu size
        // Each menu item is 30px height
        // Menu has 10px padding top and bottom
        let menu_height = (self.menu_items.len() * 30 + 20) as f64 * scale_factor;
        let menu_width = 200.0 * scale_factor;
        let size = Size2D::new(menu_width as i32, menu_height as i32);
        let rect = DeviceIntRect::from_origin_and_size(origin, size);

        self.webview.set_size(rect);
    }

    /// get item json
    fn to_items_json(&self) -> String {
        serde_json::to_string(&self.menu_items).unwrap()
    }
}

/// Menu Item
#[derive(Debug, Clone, Serialize)]
pub struct MenuItem {
    id: String,
    /// label of the menu item
    pub label: String,
    /// Whether the menu item is enabled
    pub enabled: bool,
}

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
pub struct ContextMenuClickResult {
    /// The id of the menu ite    /// Get the label of the menu item
    pub id: String,
    /// Close the context menu
    pub close: bool,
}
