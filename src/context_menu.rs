use muda::{ContextMenu, Menu, MenuItem};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Context Menu
pub struct VersoContextMenu {
    menu: Menu,
}

impl VersoContextMenu {
    /// new ContextMenu
    pub fn new() -> Self {
        let menu = Menu::new();
        let back = MenuItem::new("Back", true, None);
        let forward = MenuItem::new("Forward", false, None);
        let reload = MenuItem::new("Reload", true, None);
        let _ = menu.append_items(&[&back, &forward, &reload]);

        Self { menu }
    }

    /// Show the context menu on current cursor position
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
                    // SAFETY: The pointer came from `WindowHandle`, which ensures
                    // that the `AppKitWindowHandle` contains a valid pointer to an
                    // `NSView`.
                    // Unwrap is fine, since the pointer came from `NonNull`.
                    // let ns_view: Id<NSView> = unsafe { Id::retain(ns_view.cast()) }.unwrap();
                    // Do something with the NSView here, like getting the `NSWindow`
                    // let ns_window = ns_view
                    //     .window()
                    //     .expect("view was not installed in a window");

                    dbg!("Showing menu...");
                    self.menu.show_context_menu_for_nsview(ns_view, None);
                }
                #[cfg(target_os = "windows")]
                RawWindowHandle::Win32(handle) => {
                    dbg!("Showing menu...");
                    let hwnd = handle.hwnd;
                    self.menu.show_context_menu_for_hwnd(hwnd.into(), None);
                }
                handle => unreachable!("unknown handle {handle:?} for platform"),
            }
        }
    }
}
