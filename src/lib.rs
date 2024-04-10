//! Verso Web Browser
//!
//! This is the documentation of Verso's types and items. See [Github page](https://github.com/european-browser/verso) for more general introduction.

#![deny(missing_docs)]

/// Main entry types and functions.
pub mod app;
/// Error and result types.
pub mod errors;
/// Utilities to read options and preferences.
pub mod prefs;
/// Utilities to access resource files
pub mod resources;
/// Utilities to write tests.
pub mod test;
/// Web view types to handle web browsing contexts and window.
pub mod webview;

pub use app::{Status, Verso};
pub use errors::{Error, Result};
/// Re-exporting `winit` for the sake of convenience.
pub use winit;
