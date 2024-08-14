//! Verso Web Browser
//!
//! This is the documentation of Verso's types and items.
//! See [GitHub repository](https://github.com/versotile-org/verso) for more general introduction.

#![deny(missing_docs)]

/// Verso's compositor component to handle webrender.
pub mod compositor;
/// Utilities to read options and preferences.
pub mod config;
/// Error and result types.
pub mod errors;
/// Utilities to handle keyboard inputs and states.
pub mod keyboard;
/// Verso's rendering context.
pub mod rendering_context;
/// Utilities to handle touch inputs and states.
pub mod touch;
/// Main entry types and functions.
pub mod verso;
/// Web view types to handle web browsing contexts.
pub mod webview;
/// Verso's window types to handle Winit's window.
pub mod window;
pub use errors::{Error, Result};
/// Utilities to write tests.
// pub mod test;
pub use verso::Verso;
/// Re-exporting Winit for the sake of convenience.
pub use winit;
