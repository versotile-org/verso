//! Verso Web Browser
//!
//! This is the documentation of Verso's types and items.
//! See [GitHub repository](https://github.com/versotile-org/verso) for more general introduction.

#![deny(missing_docs)]

/// Utilities to read options and preferences.
pub mod config;
/// Error and result types.
pub mod errors;
/// Utilities to handle keyboard inputs and states.
pub mod keyboard;
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
/// Verso instance

/// Status of Verso instance.
#[derive(Clone, Copy, Debug, Default)]
pub enum Status {
    /// Nothing important to Verso at the moment.
    #[default]
    None,
    /// One of the WebViews is animating.
    Animating,
    /// Verso has shut down.
    Shutdown,
}
