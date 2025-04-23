use std::collections::HashMap;

use crate::webview::{WebView, prompt::PromptDialog};
use base::id::WebViewId;
use serde::{Deserialize, Serialize};
use servo_url::ServoUrl;
use webrender_api::units::DeviceRect;

/// Tab state
pub struct Tab {
    /// Tab WebView id
    id: WebViewId,
    /// Tab WebView
    webview: WebView,
    /// History
    history: TabHistory,
    /// Prompt
    prompt: Option<PromptDialog>,
    /// Title
    title: String,
}

impl Tab {
    /// Create a new tab state.
    pub fn new(webview: WebView) -> Self {
        Self {
            id: webview.webview_id,
            webview,
            history: TabHistory {
                list: Vec::new(),
                current_idx: 0,
            },
            prompt: None,
            title: "null".to_string(),
        }
    }

    /// Get tab WebView id.
    pub fn id(&self) -> WebViewId {
        self.id
    }

    /// Get tab WebView.
    pub fn webview(&self) -> &WebView {
        &self.webview
    }

    /// Set tab WebView size.
    pub fn set_webview_size(&mut self, rect: DeviceRect) {
        self.webview.set_size(rect);
    }

    /// Get tab history.
    pub fn history(&self) -> &TabHistory {
        &self.history
    }

    /// Set tab history.
    pub fn set_history(&mut self, list: Vec<ServoUrl>, current_idx: usize) {
        self.history = TabHistory { list, current_idx };
    }

    /// Get tab prompt dialog.
    pub fn prompt(&self) -> Option<&PromptDialog> {
        self.prompt.as_ref()
    }

    /// Get tab prompt id.
    pub fn prompt_id(&self) -> Option<WebViewId> {
        self.prompt.as_ref().map(|p| p.id())
    }

    /// Set tab prompt dialog.
    pub fn set_prompt(&mut self, prompt: PromptDialog) {
        self.prompt = Some(prompt);
    }

    /// Remove tab prompt dialog.
    pub fn remove_prompt(&mut self) -> Option<PromptDialog> {
        self.prompt.take()
    }

    /// Check if there is a prompt dialog.
    pub fn has_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    /// Set prompt webview size.
    pub fn set_prompt_size(&mut self, rect: DeviceRect) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_size(rect);
        }
    }

    /// Set tab title.
    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    /// Get tab title.
    pub fn title(&self) -> String {
        self.title.clone()
    }
}

/// Tab manager to handle multiple tab in a window.
pub struct TabManager {
    /// Current active tab id
    active_tab_id: Option<WebViewId>,
    /// Tab webview id -> Tab webview
    tab_map: HashMap<WebViewId, Tab>,
    /// Prompt webview id -> Parent tab webview id
    prompt_tab_map: HashMap<WebViewId, WebViewId>,
}

impl TabManager {
    /// Create a new tab manager.
    pub fn new() -> Self {
        Self {
            active_tab_id: None,
            tab_map: HashMap::new(),
            prompt_tab_map: HashMap::new(),
        }
    }
    /// Get tab count.
    pub fn count(&self) -> usize {
        self.tab_map.len()
    }
    /// Get current actvie tab id.
    pub fn current_tab_id(&self) -> Option<WebViewId> {
        self.active_tab_id
    }
    /// Get current active tab.
    pub fn current_tab(&self) -> Option<&Tab> {
        if let Some(tab_id) = self.active_tab_id {
            self.tab_map.get(&tab_id)
        } else {
            None
        }
    }
    /// Get current active tab as mutable.
    pub fn current_tab_mut(&mut self) -> Option<&mut Tab> {
        if let Some(tab_id) = self.active_tab_id {
            self.tab_map.get_mut(&tab_id)
        } else {
            None
        }
    }
    /// Get all tab id.
    pub fn tab_ids(&self) -> Vec<WebViewId> {
        self.tab_map.keys().cloned().collect()
    }
    /// Activate the tab by tab id.
    pub fn activate_tab(&mut self, tab_id: WebViewId) -> Option<&Tab> {
        if let Some(tab) = self.tab_map.get(&tab_id) {
            self.active_tab_id = Some(tab_id);
            Some(tab)
        } else {
            self.active_tab_id = None;
            None
        }
    }
    /// Get tab by tab id.
    pub fn tab(&self, id: WebViewId) -> Option<&Tab> {
        self.tab_map.get(&id)
    }
    /// Append a tab.
    pub fn append_tab(&mut self, webview: WebView, active: bool) {
        let id = webview.webview_id;
        let tab = Tab::new(webview);
        self.tab_map.insert(id, tab);
        if active {
            self.active_tab_id = Some(id);
        }
    }
    /// Close a tab.
    pub fn close_tab(&mut self, id: WebViewId) -> Result<Tab, TabManagerErr> {
        match self.tab_map.remove(&id) {
            Some(tab) => Ok(tab),
            None => Err(TabManagerErr::WebViewIdNotFound),
        }
    }
    /// Set tab size. Will also set prompt dialog size if it exists.
    /// - Returns the tab and prompt WebViewId if they exist.
    pub fn set_size(
        &mut self,
        tab_id: WebViewId,
        rect: DeviceRect,
    ) -> (Option<WebViewId>, Option<WebViewId>) {
        if let Some(tab) = self.tab_map.get_mut(&tab_id) {
            tab.set_webview_size(rect);

            if let Some(prompt_id) = tab.prompt_id() {
                tab.set_prompt_size(rect);
                (Some(tab_id), Some(prompt_id))
            } else {
                (Some(tab_id), None)
            }
        } else {
            (None, None)
        }
    }

    /* History */

    /// Get tab history.
    pub fn history(&self, tab_id: WebViewId) -> Option<&TabHistory> {
        self.tab_map.get(&tab_id).map(|tab| tab.history())
    }
    /// Set tab history.
    pub fn set_history(&mut self, tab_id: WebViewId, list: Vec<ServoUrl>, current_idx: usize) {
        if let Some(tab) = self.tab_map.get_mut(&tab_id) {
            tab.set_history(list, current_idx);
        };
    }

    /* Prompt */

    /// Get prompt dialog by tab id.
    pub fn prompt_by_tab_id(&self, tab_id: WebViewId) -> Option<&PromptDialog> {
        self.tab_map.get(&tab_id).and_then(|tab| tab.prompt())
    }
    /// Get prompt dialog by tab id.
    pub fn prompt_by_prompt_id(&self, prompt_id: WebViewId) -> Option<&PromptDialog> {
        if let Some(tab_id) = self.prompt_tab_map.get(&prompt_id) {
            self.prompt_by_tab_id(*tab_id)
        } else {
            None
        }
    }
    /// Get current tabw prompt dialog.
    pub fn current_prompt(&self) -> Option<&PromptDialog> {
        if let Some(tab_id) = self.active_tab_id {
            self.prompt_by_tab_id(tab_id)
        } else {
            None
        }
    }
    /// Set tab prompt dialog.
    pub fn set_prompt(&mut self, tab_id: WebViewId, prompt: PromptDialog) {
        if let Some(tab) = self.tab_map.get_mut(&tab_id) {
            self.prompt_tab_map.insert(prompt.id(), tab_id);
            tab.set_prompt(prompt);
        }
    }
    /// Remove prompt by tab webview ID.
    pub fn remove_prompt_by_tab_id(&mut self, tab_id: WebViewId) -> Option<PromptDialog> {
        if let Some(tab) = self.tab_map.get_mut(&tab_id) {
            if let Some(prompt) = tab.remove_prompt() {
                self.prompt_tab_map.remove(&prompt.id());
                return Some(prompt);
            }
        }
        None
    }
    /// Remove prompt by prompt webview ID.
    pub fn remove_prompt_by_prompt_id(&mut self, prompt_id: WebViewId) -> Option<PromptDialog> {
        if let Some(tab_id) = self.prompt_tab_map.remove(&prompt_id) {
            self.remove_prompt_by_tab_id(tab_id)
        } else {
            None
        }
    }
    /// Check if there is a prompt dialog by prompt webview ID.
    pub fn has_prompt(&self, prompt_id: WebViewId) -> bool {
        self.prompt_tab_map.contains_key(&prompt_id)
    }
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Tab history
pub struct TabHistory {
    /// History list
    pub list: Vec<ServoUrl>,
    /// Current index
    pub current_idx: usize,
}

/// Tab manager errors.
pub enum TabManagerErr {
    /// Index out of bounds.
    IndexOutOfBounds,
    /// WebView WebViewId not found.
    WebViewIdNotFound,
    /// Remove last WebView.
    RemoveLastWebView,
}

/// Response to UI that the tab was created.
#[derive(Debug, Clone, Serialize)]
pub struct TabCreateResponse {
    /// Tab creation success
    pub success: bool,
    /// Tab WebView id
    pub id: WebViewId,
}

impl TabCreateResponse {
    /// Create a new TabCreatedResult json string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

/// Activate the tab request from UI.
#[derive(Debug, Clone, Deserialize)]
pub struct TabActivateRequest {
    /// Tab WebView id
    pub id: WebViewId,
}

/// Activate the tab request from UI.
#[derive(Debug, Clone, Deserialize)]
pub struct TabCloseRequest {
    /// Tab WebView id
    pub id: WebViewId,
}
