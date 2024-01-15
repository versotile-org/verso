use raw_window_handle::HasRawWindowHandle;
use url::Url;
use winit::{event_loop::EventLoopProxy, window::Window};

use crate::{Rect, Result, WebContext, WebViewAttributes, WebViewBuilder, RGBA};

use self::{
  embedder::{Embedder, EmbedderWaker},
  window::WebView,
};

mod embedder;
mod prefs;
mod resources;
mod window;

pub(crate) struct InnerWebView {
  embedder: Embedder,
}

impl InnerWebView {
  pub fn new_servo(
    window: Window,
    proxy: EventLoopProxy<()>,
    attributes: WebViewAttributes,
    pl_attrs: super::PlatformSpecificWebViewAttributes,
    web_context: Option<&mut WebContext>,
  ) -> Result<Self> {
    resources::init(web_context);
    prefs::init();

    let webview = WebView::new(window);
    let callback = EmbedderWaker(proxy);
    let embedder = Embedder::new(webview, callback);

    Ok(Self { embedder })
  }

  pub fn new<W: HasRawWindowHandle>(
    _window: &W,
    _attributes: WebViewAttributes,
    _pl_attrs: super::PlatformSpecificWebViewAttributes,
    _web_context: Option<&mut WebContext>,
  ) -> Result<Self> {
    todo!()
  }

  pub fn new_as_child<W: HasRawWindowHandle>(
    _parent: &W,
    _attributes: WebViewAttributes,
    _pl_attrs: super::PlatformSpecificWebViewAttributes,
    _web_context: Option<&mut WebContext>,
  ) -> Result<Self> {
    todo!()
  }

  pub fn print(&self) {}

  pub fn url(&self) -> Url {
    Url::parse("").unwrap()
  }

  pub fn eval(
    &self,
    js: &str,
    callback: Option<impl FnOnce(String) + Send + 'static>,
  ) -> Result<()> {
    Ok(())
  }

  #[cfg(any(debug_assertions, feature = "devtools"))]
  pub fn open_devtools(&self) {}

  #[cfg(any(debug_assertions, feature = "devtools"))]
  pub fn close_devtools(&self) {}

  #[cfg(any(debug_assertions, feature = "devtools"))]
  pub fn is_devtools_open(&self) -> bool {
    true
  }

  pub fn zoom(&self, scale_factor: f64) {}

  pub fn set_background_color(&self, background_color: RGBA) -> Result<()> {
    Ok(())
  }

  pub fn load_url(&self, url: &str) {}

  pub fn load_url_with_headers(&self, url: &str, headers: http::HeaderMap) {}

  pub fn clear_all_browsing_data(&self) -> Result<()> {
    Ok(())
  }

  pub fn set_bounds(&self, bounds: Rect) {}

  pub fn set_visible(&self, visible: bool) {}

  pub fn focus(&self) {}
}

pub fn platform_webview_version() -> Result<String> {
  Ok(String::from(""))
}

pub trait WebViewBuilderExtServo {
  fn new_servo(window: Window, proxy: EventLoopProxy<()>) -> Self;
}

impl WebViewBuilderExtServo for WebViewBuilder<'_> {
  fn new_servo(window: Window, proxy: EventLoopProxy<()>) -> Self {
    Self {
      attrs: WebViewAttributes::default(),
      window: None,
      as_child: false,
      #[allow(clippy::default_constructed_unit_structs)]
      platform_specific: super::PlatformSpecificWebViewAttributes::default(),
      web_context: None,
      winit: Some((window, proxy)),
    }
  }
}

pub trait WebViewExtServo {
  fn servo(&mut self) -> &mut Embedder; // TODO expose method instead.
}

impl WebViewExtServo for super::WebView {
  fn servo(&mut self) -> &mut Embedder {
    &mut self.webview.embedder
  }
}
