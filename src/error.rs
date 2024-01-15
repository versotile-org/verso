/// Convenient type alias of Result type for wry.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by wry.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum Error {
  #[cfg(gtk)]
  #[error(transparent)]
  GlibError(#[from] gtk::glib::Error),
  #[cfg(gtk)]
  #[error(transparent)]
  GlibBoolError(#[from] gtk::glib::BoolError),
  #[cfg(gtk)]
  #[error("Fail to fetch security manager")]
  MissingManager,
  #[cfg(gtk)]
  #[error("Couldn't find X11 Display")]
  X11DisplayNotFound,
  #[cfg(gtk)]
  #[error(transparent)]
  XlibError(#[from] x11_dl::error::OpenError),
  #[error("Failed to initialize the script")]
  InitScriptError,
  #[error("Bad RPC request: {0} ((1))")]
  RpcScriptError(String, String),
  #[error(transparent)]
  NulError(#[from] std::ffi::NulError),
  #[error(transparent)]
  ReceiverError(#[from] std::sync::mpsc::RecvError),
  #[error(transparent)]
  SenderError(#[from] std::sync::mpsc::SendError<String>),
  #[error("Failed to send the message")]
  MessageSender,
  #[error(transparent)]
  Json(#[from] serde_json::Error),
  #[error(transparent)]
  UrlError(#[from] url::ParseError),
  #[error("IO error: {0}")]
  Io(#[from] std::io::Error),
  #[cfg(target_os = "windows")]
  #[error("WebView2 error: {0}")]
  WebView2Error(webview2_com::Error),
  #[error("Duplicate custom protocol registered: {0}")]
  DuplicateCustomProtocol(String),
  #[error(transparent)]
  HttpError(#[from] http::Error),
  #[error("Infallible error, something went really wrong: {0}")]
  Infallible(#[from] std::convert::Infallible),
  #[cfg(target_os = "android")]
  #[error(transparent)]
  JniError(#[from] jni::errors::Error),
  #[error("Failed to create proxy endpoint")]
  ProxyEndpointCreationFailed,
  #[error(transparent)]
  WindowHandleError(#[from] raw_window_handle::HandleError),
  #[error("the window handle kind is not supported")]
  UnsupportedWindowHandle,
  #[error(transparent)]
  Utf8Error(#[from] std::str::Utf8Error),
}
