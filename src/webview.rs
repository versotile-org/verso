use servo::{
    compositing::windowing::EmbedderEvent, embedder_traits::EmbedderMsg,
    msg::constellation_msg::WebViewId,
};

/// A web view is an area to display web browsing context. It's what user will treat as a "web page".
pub struct WebView {
    id: Option<WebViewId>,
}

impl WebView {
    /// Create a web view from winit window.
    pub fn new() -> Self {
        Self { id: None }
    }

    /// Set web view ID of this window.
    pub fn set_id(&mut self, id: WebViewId) {
        self.id = Some(id);
    }

    /// Get web view ID of this window.
    pub fn id(&self) -> &Option<WebViewId> {
        &self.id
    }

    /// Handle servo messages and return a boolean to indicate servo needs to present or not.
    pub fn handle_servo_messages(&self, events: &mut Vec<EmbedderEvent>, message: EmbedderMsg) {
        log::trace!(
            "Verso WebView {:?} is handling servo message: {:?}",
            self.id,
            message
        );
        match message {
            EmbedderMsg::WebViewOpened(w) => {
                events.push(EmbedderEvent::FocusWebView(w));
            }
            EmbedderMsg::AllowNavigationRequest(pipeline_id, _url) => {
                events.push(EmbedderEvent::AllowNavigationResponse(pipeline_id, true));
            }
            EmbedderMsg::WebViewClosed(_w) => {
                events.push(EmbedderEvent::Quit);
            }
            e => {
                log::warn!("Verso WebView hasn't supported handling this message yet: {e:?}")
            }
        }
    }
}
