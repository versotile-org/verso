#[cfg(any(target_os = "macos", target_os = "windows"))]
use muda::{ContextMenu as MudaContextMenu, Menu};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Context Menu
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct ContextMenu {
    menu: Menu,
}

/// Context Menu
#[cfg(linux)]
pub struct ContextMenu {}

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
