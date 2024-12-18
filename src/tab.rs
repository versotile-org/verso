use std::collections::HashMap;

use crate::webview::{prompt::PromptDialog, WebView};
use base::id::WebViewId;
use serde::{Deserialize, Serialize};
use servo_url::ServoUrl;
use webrender_api::units::DeviceIntRect;

/// Tab manager to handle multiple WebViews in a window.
pub struct TabManager {
    /// WebViews in the tab.
    tab_id_order: Vec<WebViewId>,
    /// Index of the active WebView.
    active_tab_id: Option<WebViewId>,
    /// Tab webview id -> Tab webview
    tab: HashMap<WebViewId, WebView>,
    /// Tab webview id -> Tab history
    history: HashMap<WebViewId, TabHistory>,
    /// Tab webview id -> Prompt dialog
    prompt: HashMap<WebViewId, PromptDialog>,
    // Prompt webview id -> Parent tab webview id
    prompt_tab_map: HashMap<WebViewId, WebViewId>,
}

impl TabManager {
    /// Create a new tab manager.
    pub fn new() -> Self {
        Self {
            tab_id_order: Vec::new(),
            active_tab_id: None,
            tab: HashMap::new(),
            history: HashMap::new(),
            prompt: HashMap::new(),
            prompt_tab_map: HashMap::new(),
        }
    }
    /// Get tab count.
    pub fn count(&self) -> usize {
        self.tab_id_order.len()
    }
    /// Get current actvie tab id.
    pub fn current_tab_id(&self) -> Option<WebViewId> {
        self.active_tab_id
    }
    /// Get the active tab.
    pub fn current_tab(&self) -> Option<&WebView> {
        if let Some(tab_id) = self.active_tab_id {
            self.tab.get(&tab_id)
        } else {
            None
        }
    }
    /// Activate the tab at the specified index.
    pub fn activate_tab(&mut self, tab_id: WebViewId) -> Option<&WebView> {
        if let Some(webview) = self.tab.get(&tab_id) {
            self.active_tab_id = Some(tab_id);
            Some(webview)
        } else {
            self.active_tab_id = None;
            None
        }
    }
    /// Get tab by tab id.
    pub fn tab(&self, id: WebViewId) -> Option<&WebView> {
        self.tab.get(&id)
    }
    /// Append a tab.
    pub fn append_tab(&mut self, webview: WebView, active: bool) {
        let id = webview.webview_id;
        self.tab.insert(id, webview);
        self.tab_id_order.push(id);
        if active {
            self.active_tab_id = Some(id);
        }
    }
    /// Close a tab.
    pub fn close_tab(&mut self, id: WebViewId) -> Result<WebView, TabManagerErr> {
        // Skip if there's only one webview
        if self.tab.len() == 1 {
            return Err(TabManagerErr::RemoveLastWebView);
        }

        if let Some(idx) = self.tab_id_order.iter().position(|tab_id| *tab_id == id) {
            self.tab_id_order.remove(idx);
        }

        self.prompt.remove(&id);
        self.history.remove(&id);
        match self.tab.remove(&id) {
            Some(webview) => Ok(webview),
            None => Err(TabManagerErr::WebViewIdNotFound),
        }
    }
    /// Close all tabs.
    pub fn close_all(&mut self) -> HashMap<WebViewId, WebView> {
        self.active_tab_id = None;
        self.history.clear();
        self.prompt.clear();
        self.tab_id_order.clear();
        self.tab.drain().collect()
    }
    /// Set tab size. Will also set prompt dialog size if it exists.
    /// - Returns the tab and prompt WebViewId if they exist.
    pub fn set_size(
        &mut self,
        id: WebViewId,
        rect: DeviceIntRect,
    ) -> (Option<WebViewId>, Option<WebViewId>) {
        let tab_id = if let Some(tab_webview) = self.tab.get_mut(&id) {
            tab_webview.set_size(rect);
            Some(id)
        } else {
            None
        };
        let prompt_id = if let Some(prompt_webview) = self.prompt.get_mut(&id) {
            prompt_webview.set_size(rect);
            Some(prompt_webview.id())
        } else {
            None
        };
        (tab_id, prompt_id)
    }

    /* History */

    /// Get tab history.
    pub fn history(&self, id: WebViewId) -> Option<&TabHistory> {
        self.history.get(&id)
    }
    /// Set tab history.
    pub fn set_history(&mut self, id: WebViewId, list: Vec<ServoUrl>, current_idx: usize) {
        self.history.insert(id, TabHistory { list, current_idx });
    }
    /// Remove tab history.
    pub fn remove_history(&mut self, id: WebViewId) {
        self.history.remove(&id);
    }

    /* Prompt */

    /// Get prompt dialog by tab id.
    pub fn prompt_by_tab_id(&self, tab_id: WebViewId) -> Option<&PromptDialog> {
        self.prompt.get(&tab_id)
    }
    /// Get prompt dialog by tab id.
    pub fn prompt_by_prompt_id(&self, prompt_id: WebViewId) -> Option<&PromptDialog> {
        if let Some(tab_id) = self.prompt_tab_map.get(&prompt_id) {
            self.prompt.get(tab_id)
        } else {
            None
        }
    }
    /// Get current tabw prompt dialog.
    pub fn current_prompt(&self) -> Option<&PromptDialog> {
        if let Some(tab_id) = self.active_tab_id {
            self.prompt.get(&tab_id)
        } else {
            None
        }
    }
    /// Set tab prompt dialog.
    pub fn set_prompt(&mut self, tab_id: WebViewId, prompt: PromptDialog) {
        let prompt_id = prompt.id();
        self.prompt.insert(tab_id, prompt);
        self.prompt_tab_map.insert(prompt_id, tab_id);
    }
    /// Remove prompt by tab webview ID.
    pub fn remove_prompt_by_tab_id(&mut self, tab_id: WebViewId) -> Option<PromptDialog> {
        if let Some(prompt) = self.prompt.remove(&tab_id) {
            self.prompt_tab_map.remove(&prompt.id());
            return Some(prompt);
        }
        None
    }
    /// Remove prompt by prompt webview ID.
    pub fn remove_prompt_by_prompt_id(&mut self, prompt_id: WebViewId) -> Option<PromptDialog> {
        if let Some(tab_id) = self.prompt_tab_map.remove(&prompt_id) {
            self.prompt.remove(&tab_id)
        } else {
            None
        }
    }
    /// Check if there is a prompt dialog by prompt webview ID.
    pub fn has_prompt(&self, prompt_id: WebViewId) -> bool {
        self.prompt_tab_map.contains_key(&prompt_id)
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
