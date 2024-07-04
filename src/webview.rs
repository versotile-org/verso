use std::cell::Cell;

use servo::{base::id::WebViewId, url::ServoUrl};

/// A web view is an area to display web browsing context. It's what user will treat as a "web page".
pub struct WebView {
    id: WebViewId,
    history: Cell<Vec<ServoUrl>>,
    current: Cell<usize>,
}

impl WebView {
    /// Create a web view from Winit window.
    pub fn new(id: WebViewId) -> Self {
        Self {
            id,
            history: Cell::new(vec![]),
            current: Cell::new(0),
        }
    }

    /// Get web view ID of this window.
    pub fn id(&self) -> WebViewId {
        self.id
    }

    /// Set the history URLs and current index of the WebView
    pub fn set_history(&self, history: Vec<ServoUrl>, current: usize) {
        self.history.replace(history);
        self.current.replace(current);
    }
}

/// A panel is a special web view that focus on controlling states around window.
/// It could be treated as the control panel or navigation bar of the window depending on usages.
///
/// At the moment, following Web API is supported:
/// - Close window: `window.close()`
/// - Navigate to previous page: `window.prompt('PREV')`
/// - Navigate to next page: `window.prompt('FORWARD')`
/// - Refresh the page: `window.prompt('REFRESH')`
/// - Minimize the window: `window.prompt('MINIMIZE')`
/// - Maximize the window: `window.prompt('MAXIMIZE')`
/// - Navigate to a specific URL: `window.prompt('NAVIGATE_TO:${url}')`
pub struct Panel {
    id: Option<WebViewId>,
}

impl Panel {
    /// Create a panel from Winit window.
    pub fn new() -> Self {
        Self { id: None }
    }

    /// Set web view ID of this panel.
    pub fn set_id(&mut self, id: WebViewId) {
        self.id = Some(id);
    }

    /// Get web view ID of this panel.
    ///
    /// We assume this is always called after `set_id`. Calling before it will cause panic.
    pub fn id(&self) -> WebViewId {
        self.id.unwrap()
    }
}
