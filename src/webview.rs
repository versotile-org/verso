use std::cell::Cell;

use servo::{msg::constellation_msg::WebViewId, url::ServoUrl};

/// A web view is an area to display web browsing context. It's what user will treat as a "web page".
pub struct WebView {
    id: Option<WebViewId>,
    history: Cell<Vec<ServoUrl>>,
    current: Cell<usize>,
}

impl WebView {
    /// Create a web view from winit window.
    pub fn new() -> Self {
        Self {
            id: None,
            history: Cell::new(vec![]),
            current: Cell::new(0),
        }
    }

    /// Set web view ID of this window.
    pub fn set_id(&mut self, id: WebViewId) {
        self.id = Some(id);
    }

    /// Get web view ID of this window.
    pub fn id(&self) -> &Option<WebViewId> {
        &self.id
    }

    /// Set the history URLs and current index of the webview
    pub fn set_history(&self, history: Vec<ServoUrl>, current: usize) {
        self.history.replace(history);
        self.current.replace(current);
    }
}
