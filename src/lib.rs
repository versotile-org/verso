//! Verso Web Browser
//!
//! This is the documentation of Verso's types and items.
//! See [GitHub repository](https://github.com/versotile-org/verso) for more general introduction.

#![deny(missing_docs)]

/// Main entry types and functions.
pub mod app;
/// Utilities to read options and preferences.
pub mod config;
/// Error and result types.
pub mod errors;
/// Utilities to handle keyboard inputs and states.
pub mod keyboard;
/// Web view types to handle web browsing contexts.
pub mod webview;
/// Verso's window types to handle Winit's window.
pub mod window;
/// Utilities to write tests.
// pub mod test;
pub use app::{Status, Verso};
pub use errors::{Error, Result};
/// Re-exporting Winit for the sake of convenience.
pub use winit;
