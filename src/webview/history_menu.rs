use super::webview_menu::WebViewMenu;
use crate::webview::WebView;
use crate::{verso::send_to_constellation, window::Window};
use base::id::WebViewId;
use constellation_traits::{EmbedderToConstellationMessage, TraversalDirection};
use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use servo_url::ServoUrl;
use std::fmt;
use url::Url;
use webrender_api::units::DeviceIntRect;
use winit::dpi::LogicalPosition;

/// The Previous/Next History Menu of the Window. It will be opened when users left long click on panel's previous/next button.
#[derive(Clone)]
pub struct HistoryMenu {
    /// The action to perform
    action: HistoryMenuAction,
    /// The menu items
    menu_items: Vec<HistoryMenuItem>,
    /// The webview that the menu is attached to
    pub(crate) webview: WebView,
    /// Menu position, used for positioning the menu by CSS
    position: LogicalPosition<f64>,
}

impl HistoryMenu {
    /// Create history menu with custom items
    pub fn new_with_menu(action: HistoryMenuAction, menu_items: Vec<HistoryMenuItem>) -> Self {
        let webview_id = WebViewId::new();
        let webview = WebView::new(webview_id, DeviceIntRect::zero());

        Self {
            action,
            menu_items,
            webview,
            position: LogicalPosition::new(0.0, 0.0),
        }
    }
}

impl WebViewMenu for HistoryMenu {
    fn webview(&self) -> &WebView {
        &self.webview
    }

    fn resource_url(&self) -> ServoUrl {
        let mut url = Url::parse("verso://resources/components/history_menu.html").unwrap();
        url.query_pairs_mut()
            .append_pair("action", &self.action.to_string());
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
        send_to_constellation(
            sender,
            EmbedderToConstellationMessage::CloseWebView(self.webview().webview_id),
        );
    }
}

impl HistoryMenu {
    /// Convert the menu items to JSON string
    fn serialize_items(&self) -> String {
        serde_json::to_string(&self.menu_items).unwrap()
    }
}

/// Menu Item
#[derive(Debug, Clone, Serialize)]
pub struct HistoryMenuItem {
    /// index of the menu item
    pub index: usize,
    /// title of the menu item
    pub title: String,
    /// url of the menu item
    pub url: String,
}

impl HistoryMenuItem {
    /// Create a new menu item
    pub fn new(index: usize, title: Option<&str>, url: &str) -> Self {
        let title = title.unwrap_or(url);
        Self {
            index,
            title: title.to_string(),
            url: url.to_string(),
        }
    }
}

/// History Menu Click Result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMenuUIResponse {
    /// index of the menu item
    pub index: Option<usize>,
    /// action of the menu item
    pub action: HistoryMenuAction,
}

/// Open History Menu Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenHistoryMenuRequest {
    /// The action to perform
    pub action: HistoryMenuAction,
    /// The position of the menu
    pub position: LogicalPosition<f64>,
}

/// Action of the history menu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HistoryMenuAction {
    /// Previous item
    Prev,
    /// Next item
    Next,
}

impl fmt::Display for HistoryMenuAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HistoryMenuAction::Prev => write!(f, "Prev"),
            HistoryMenuAction::Next => write!(f, "Next"),
        }
    }
}

impl Window {
    pub(crate) fn show_history_menu(
        &mut self,
        sender: &Sender<EmbedderToConstellationMessage>,
        request: OpenHistoryMenuRequest,
    ) -> Option<HistoryMenu> {
        let tab = self.tab_manager.current_tab().unwrap();
        let history = tab.history();
        let current_index = history.current_idx;
        let items: Vec<HistoryMenuItem> = match request.action {
            HistoryMenuAction::Prev => {
                // For prev, we want items from current_index-1 to 0
                history.list[..current_index]
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(index, url)| HistoryMenuItem::new(index, None, url.as_str()))
                    .collect()
            }
            HistoryMenuAction::Next => {
                // For next, we want items from current_index+1 to end
                history.list[current_index + 1..]
                    .iter()
                    .enumerate()
                    .map(|(index, url)| HistoryMenuItem::new(index, None, url.as_str()))
                    .collect()
            }
        };

        if items.is_empty() {
            return None;
        }

        let mut menu = HistoryMenu::new_with_menu(request.action, items);
        menu.show(
            sender,
            self,
            request.position.to_physical(self.scale_factor()),
        );
        Some(menu)
    }

    pub(crate) fn handle_history_menu_event(
        &mut self,
        sender: &Sender<EmbedderToConstellationMessage>,
        event: crate::webview::history_menu::HistoryMenuUIResponse,
    ) {
        self.close_webview_menu(sender);

        if let Some(tab_id) = self.tab_manager.current_tab_id() {
            if let Some(index) = event.index {
                match event.action {
                    HistoryMenuAction::Prev => {
                        send_to_constellation(
                            sender,
                            EmbedderToConstellationMessage::TraverseHistory(
                                tab_id,
                                TraversalDirection::Back(index + 1),
                            ),
                        );
                    }
                    HistoryMenuAction::Next => {
                        send_to_constellation(
                            sender,
                            EmbedderToConstellationMessage::TraverseHistory(
                                tab_id,
                                TraversalDirection::Forward(index + 1),
                            ),
                        );
                    }
                }
            }
        } else {
            log::error!("No active webview to handle history menu event");
        }
    }
}
