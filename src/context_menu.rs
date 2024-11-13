use base::id::WebViewId;
use euclid::{Point2D, Size2D};

/* macOS, Windows Native Implementation */
#[cfg(any(target_os = "macos", target_os = "windows"))]
use muda::{ContextMenu as MudaContextMenu, Menu};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/* Wayland Implementation */
#[cfg(linux)]
use crate::webview::WebView;
#[cfg(linux)]
use serde::{Deserialize, Serialize};
#[cfg(linux)]
use webrender_api::units::DeviceIntPoint;
#[cfg(linux)]
use webrender_api::units::DeviceIntRect;

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
    pub webview: Option<WebView>,
}

#[cfg(linux)]
impl ContextMenu {
    /// Create a dialog in the window.
    ///
    /// Often used by calling window.alert() or window.confirm() in the web page.
    pub fn new_with_menu(menu_items: Vec<MenuItem>) -> Self {
        Self {
            menu_items,
            webview: None,
        }
    }
    /// Set the context menu's options
    pub fn set_menu_items(&mut self, menu_items: Vec<MenuItem>) {
        self.menu_items = menu_items;
    }
    /// Show the context menu on position
    pub fn create_webview(&mut self, position: DeviceIntPoint, scale_factor: f64) -> WebView {
        // Translate position to origin
        let origin = Point2D::new(position.x, position.y);

        // Calculate menu size
        // Each menu item is 30px height
        // Menu has 10px padding top and bottom
        let menu_height = (self.menu_items.len() * 30 + 20) as f64 * scale_factor;
        let menu_width = 200.0 * scale_factor;
        let size = Size2D::new(menu_width as i32, menu_height as i32);
        let rect = DeviceIntRect::from_origin_and_size(origin, size);

        let webview_id = WebViewId::new();
        let webview = WebView::new(webview_id, rect);
        // let url = ServoUrl::parse("https://example.com").unwrap();

        self.webview = Some(webview.clone());

        webview
    }

    /// get item json
    pub fn get_items_json(&self) -> String {
        serde_json::to_string(&self.menu_items).unwrap()
    }
}

/// Menu Item
#[derive(Debug, Clone, Serialize)]
pub struct MenuItem {
    id: String,
    label: String,
    enabled: bool,
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
    /// Get the label of the menu item
    pub fn label(&self) -> &str {
        &self.label
    }
    /// Set the label of the menu item
    pub fn set_label(&mut self, label: &str) -> &Self {
        self.label = label.to_string();
        self
    }
    /// Enable or disable menu item
    pub fn set_enabled(&mut self, enabled: bool) -> &Self {
        self.enabled = enabled;
        self
    }
}

/// Context Menu Click Result
#[cfg(linux)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMenuClickResult {
    /// The id of the menu item
    pub id: String,
    /// Close the context menu
    pub close: bool,
}
