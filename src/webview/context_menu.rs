use crate::verso::send_to_constellation;
use crate::window::Window;
use constellation_traits::{EmbedderToConstellationMessage, TraversalDirection};
use embedder_traits::ContextMenuResult;
use ipc_channel::ipc::IpcSender;

/* macOS, Windows Native Implementation */
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crossbeam_channel::Sender;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use muda::MenuEvent;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use muda::{ContextMenu as MudaContextMenu, Menu as MudaMenu};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/* Wayland Implementation */
#[cfg(linux)]
use super::webview_menu::WebViewMenu;
#[cfg(linux)]
use crate::webview::WebView;
#[cfg(linux)]
use base::id::WebViewId;
#[cfg(linux)]
use crossbeam_channel::Sender;
#[cfg(linux)]
use serde::{Deserialize, Serialize};
#[cfg(linux)]
use servo_url::ServoUrl;
#[cfg(linux)]
use url::Url;
#[cfg(linux)]
use webrender_api::units::DeviceIntRect;
#[cfg(linux)]
use winit::dpi::LogicalPosition;

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
impl WebViewMenu for ContextMenu {
    /// Get webview of the context menu
    fn webview(&self) -> &WebView {
        &self.webview
    }

    /// Get resource URL of the context menu
    fn resource_url(&self) -> ServoUrl {
        let mut url = Url::parse("verso://resources/components/context_menu.html").unwrap();
        url.query_pairs_mut()
            .append_pair("items", &self.serialize_items());
        url.query_pairs_mut()
            .append_pair("pos_x", &self.position.x.to_string());
        url.query_pairs_mut()
            .append_pair("pos_y", &self.position.y.to_string());
        ServoUrl::from_url(url)
    }

    fn set_webview_rect(&mut self, rect: DeviceIntRect) {
        self.webview.set_size(rect);
    }

    fn position(&self) -> LogicalPosition<f64> {
        self.position
    }

    fn set_position(&mut self, position: LogicalPosition<f64>) {
        self.position = position;
    }

    fn close(&mut self, sender: &Sender<EmbedderToConstellationMessage>) {
        self.send_result_to_servo(ContextMenuResult::Dismissed);
        send_to_constellation(
            sender,
            EmbedderToConstellationMessage::CloseWebView(self.webview().webview_id),
        );
    }
}

#[cfg(linux)]
impl ContextMenu {
    /// Convert the context menu items to JSON string
    fn serialize_items(&self) -> String {
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

// Context Menu methods
impl Window {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) fn show_context_menu(
        &self,
        servo_sender: IpcSender<ContextMenuResult>,
    ) -> ContextMenu {
        use muda::MenuItem;

        let tab = self.tab_manager.current_tab().unwrap();
        let history = tab.history();
        let history_len = history.list.len();

        // items
        let back = MenuItem::with_id("back", "Back", history.current_idx > 0, None);
        let forward = MenuItem::with_id(
            "forward",
            "Forward",
            history.current_idx + 1 < history_len,
            None,
        );
        let reload = MenuItem::with_id("reload", "Reload", true, None);

        let menu = MudaMenu::new();
        let _ = menu.append_items(&[&back, &forward, &reload]);

        let context_menu = ContextMenu::new_with_menu(servo_sender, Menu(menu));
        context_menu.show(self.window.window_handle().unwrap());

        context_menu
    }

    #[cfg(linux)]
    pub(crate) fn show_context_menu(
        &mut self,
        sender: &Sender<EmbedderToConstellationMessage>,
        servo_sender: IpcSender<ContextMenuResult>,
    ) -> ContextMenu {
        let tab = self.tab_manager.current_tab().unwrap();
        let history = tab.history();
        let history_len = history.list.len();

        // items
        let back = MenuItem::new(Some("back"), "Back", history.current_idx > 0);
        let forward = MenuItem::new(
            Some("forward"),
            "Forward",
            history.current_idx + 1 < history_len,
        );
        let reload = MenuItem::new(Some("reload"), "Reload", true);

        let mut context_menu =
            ContextMenu::new_with_menu(servo_sender, Menu(vec![back, forward, reload]));

        let position = self.mouse_position.get().unwrap();
        context_menu.show(sender, self, position);

        context_menu
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) fn handle_context_menu_event(
        &self,
        mut context_menu: ContextMenu,
        sender: &Sender<EmbedderToConstellationMessage>,
        event: MenuEvent,
    ) {
        context_menu.send_result_to_servo(ContextMenuResult::Dismissed);
        // TODO: should be more flexible to handle different menu items
        let active_tab = self.tab_manager.current_tab().unwrap();
        match event.id().0.as_str() {
            "back" => {
                send_to_constellation(
                    sender,
                    EmbedderToConstellationMessage::TraverseHistory(
                        active_tab.id(),
                        TraversalDirection::Back(1),
                    ),
                );
            }
            "forward" => {
                send_to_constellation(
                    sender,
                    EmbedderToConstellationMessage::TraverseHistory(
                        active_tab.id(),
                        TraversalDirection::Forward(1),
                    ),
                );
            }
            "reload" => {
                send_to_constellation(
                    sender,
                    EmbedderToConstellationMessage::Reload(active_tab.id()),
                );
            }
            _ => {}
        }
    }

    /// Handle linux context menu event
    // TODO(context-menu): should make the call in synchronous way after calling show_context_menu, otherwise
    // we'll have to deal with constellation sender and other parameter's lifetime, also we lose the context that why this context menu popup
    #[cfg(linux)]
    pub(crate) fn handle_context_menu_event(
        &mut self,
        sender: &Sender<EmbedderToConstellationMessage>,
        event: crate::webview::context_menu::ContextMenuUIResponse,
    ) {
        self.close_webview_menu(sender);
        if let Some(id) = event.id {
            if let Some(tab_id) = self.tab_manager.current_tab_id() {
                match id.as_str() {
                    "back" => {
                        send_to_constellation(
                            sender,
                            EmbedderToConstellationMessage::TraverseHistory(
                                tab_id,
                                TraversalDirection::Back(1),
                            ),
                        );
                    }
                    "forward" => {
                        send_to_constellation(
                            sender,
                            EmbedderToConstellationMessage::TraverseHistory(
                                tab_id,
                                TraversalDirection::Forward(1),
                            ),
                        );
                    }
                    "reload" => {
                        send_to_constellation(
                            sender,
                            EmbedderToConstellationMessage::Reload(tab_id),
                        );
                    }
                    _ => {}
                }
            } else {
                log::error!("No active webview to handle context menu event");
            }
        };
    }
}
