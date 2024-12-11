use std::collections::HashMap;

use crate::webview::WebView;
use base::id::{TopLevelBrowsingContextId, WebViewId};
use serde::{Deserialize, Serialize};
use servo_url::ServoUrl;

/// Tab manager to handle multiple WebViews in a window.
pub struct TabManager {
    /// WebViews in the tab.
    webviews: Vec<WebView>,
    /// Index of the active WebView.
    active_idx: usize,
    history: HashMap<WebViewId, TabHistory>,
}

impl TabManager {
    /// Create a new tab manager.
    pub fn new() -> Self {
        Self {
            active_idx: 0,
            webviews: Vec::new(),
            history: HashMap::new(),
        }
    }
    /// Get opened WebViews count.
    pub fn count(&self) -> usize {
        self.webviews.len()
    }
    /// Get the active WebView.
    pub fn active_webview(&self) -> Option<&WebView> {
        if self.webviews.is_empty() {
            None
        } else {
            Some(&self.webviews[self.active_idx])
        }
    }
    /// Get the active WebView.
    pub fn active_webview_as_mut(&mut self) -> Option<&mut WebView> {
        if self.webviews.is_empty() {
            None
        } else {
            Some(&mut self.webviews[self.active_idx])
        }
    }
    /// Get webvies as mutable
    pub fn webviews_as_mut(&mut self) -> &mut Vec<WebView> {
        &mut self.webviews
    }
    /// Get webvies as mutable
    pub fn webviews_by_id_as_mut(&mut self, id: TopLevelBrowsingContextId) -> Option<&mut WebView> {
        self.webviews
            .iter_mut()
            .find(|webview| webview.webview_id == id)
    }
    /// Get active WebView index.
    pub fn active_idx(&self) -> usize {
        self.active_idx
    }
    /// Activate the WebView at the specified index.
    pub fn activate_webview(&mut self, idx: usize) -> Option<&WebView> {
        if let Some(webview) = self.webviews.get(idx) {
            self.active_idx = idx;
            Some(webview)
        } else {
            None
        }
    }
    /// Activate the WebView at the specified index.
    pub fn activate_webview_by_id(&mut self, id: TopLevelBrowsingContextId) -> Option<&WebView> {
        if let Some((idx, webview)) = self
            .webviews
            .iter()
            .enumerate()
            .find(|(_, webview)| webview.webview_id == id)
        {
            self.active_idx = idx;
            Some(webview)
        } else {
            None
        }
    }
    /// Get WebView at the specified index.
    pub fn webview_at(&self, idx: usize) -> Option<&WebView> {
        self.webviews.get(idx)
    }
    /// Get WebView by WebViewId.
    pub fn webview_by_id(&self, id: TopLevelBrowsingContextId) -> Option<&WebView> {
        self.webviews
            .iter()
            .find(|webview| webview.webview_id == id)
    }
    /// Append a WebView to the end of the tabs.
    pub fn append_webview(&mut self, webview: WebView, active: bool) {
        self.webviews.push(webview);
        if active {
            self.active_idx = self.webviews.len() - 1;
        }
    }
    /// Insert a WebView at the specified index.
    pub fn insert_webview(&mut self, idx: usize, webview: WebView, active: bool) {
        self.webviews.insert(idx, webview);
        if active {
            self.active_idx = idx;
        }
    }
    /// Remove a WebView at the specified index.
    pub fn remove_webview_at(&mut self, idx: usize) -> Result<WebView, TabManagerErr> {
        // Skip if there's only one webview
        if self.webviews.len() == 1 {
            return Err(TabManagerErr::RemoveLastWebView);
        }
        if idx >= self.webviews.len() {
            return Err(TabManagerErr::IndexOutOfBounds);
        }

        let webview = self.webviews.remove(idx);
        self.remove_history(webview.webview_id);

        if idx < self.active_idx {
            self.active_idx -= 1;
        } else if idx >= self.webviews.len() {
            self.active_idx = self.webviews.len() - 1;
        }

        Ok(webview)
    }
    /// Remove a WebView at the specified index.
    pub fn remove_webview_by_id(
        &mut self,
        id: TopLevelBrowsingContextId,
    ) -> Result<WebView, TabManagerErr> {
        // Skip if there's only one webview
        if self.webviews.len() == 1 {
            return Err(TabManagerErr::RemoveLastWebView);
        }

        if let Some(idx) = self
            .webviews
            .iter()
            .position(|webview| webview.webview_id == id)
        {
            return self.remove_webview_at(idx);
        }

        Err(TabManagerErr::WebViewIdNotFound)
    }
    /// Close all tabs.
    pub fn close_all(&mut self) -> Vec<WebView> {
        self.active_idx = 0;
        self.history.clear();
        self.webviews.drain(..).collect()
    }
    /// Set webview history.
    pub fn set_history(&mut self, id: WebViewId, list: Vec<ServoUrl>, current_idx: usize) {
        self.history.insert(id, TabHistory { list, current_idx });
    }
    /// Set webview history.
    pub fn history(&self, id: WebViewId) -> Option<&TabHistory> {
        self.history.get(&id)
    }
    /// Remove webview history.
    pub fn remove_history(&mut self, id: WebViewId) {
        self.history.remove(&id);
    }
}

/// Tab manager errors.
pub enum TabManagerErr {
    /// Index out of bounds.
    IndexOutOfBounds,
    /// WebView TopLevelBrowsingContextId not found.
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
    pub id: TopLevelBrowsingContextId,
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
    pub id: TopLevelBrowsingContextId,
}

/// Activate the tab request from UI.
#[derive(Debug, Clone, Deserialize)]
pub struct TabCloseRequest {
    /// Tab WebView id
    pub id: TopLevelBrowsingContextId,
}

/// Tab history
pub struct TabHistory {
    /// History list
    pub list: Vec<ServoUrl>,
    /// Current index
    pub current_idx: usize,
}
