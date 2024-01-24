use winit::{event_loop::EventLoopProxy, window::Window};

use self::{
  embedder::{Embedder, EmbedderWaker},
  webview::WebView,
};

mod embedder;
mod prefs;
mod resources;
mod webview;

pub struct Yippee {
  embedder: Embedder,
}

impl Yippee {
  pub fn new(
    window: Window,
    proxy: EventLoopProxy<()>,
  ) -> Self {
    resources::init();
    prefs::init();

    let webview = WebView::new(window);
    let callback = EmbedderWaker(proxy);
    let embedder = Embedder::new(webview, callback);

    Self { embedder }
  }

  pub fn embedder(&mut self) -> &mut Embedder {
    &mut self.embedder
  }
}
