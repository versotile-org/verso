use dpi::{Position, Size};
use std::path::{Path, PathBuf};
use versoview_messages::{ConfigFromController, ProfilerSettings};

use crate::VersoviewController;

/// A builder for configuring and creating a `VersoviewController` instance.
pub struct VersoBuilder(ConfigFromController);

impl VersoBuilder {
    /// Creates a new `VersoBuilder` with default settings.
    pub fn new() -> Self {
        Self(ConfigFromController::default())
    }

    /// Sets whether the control panel should be included.
    pub fn with_panel(mut self, with_panel: bool) -> Self {
        self.0.with_panel = with_panel;
        self
    }

    /// Sets the initial window size.
    pub fn inner_size(mut self, size: impl Into<Size>) -> Self {
        self.0.inner_size = Some(size.into());
        self
    }

    /// Sets the initial window position.
    pub fn position(mut self, position: impl Into<Position>) -> Self {
        self.0.position = Some(position.into());
        self
    }

    /// Sets whether the window should start maximized.
    pub fn maximized(mut self, maximized: bool) -> Self {
        self.0.maximized = maximized;
        self
    }

    /// Sets whether the window should be visible initially.
    pub fn visible(mut self, visible: bool) -> Self {
        self.0.visible = visible;
        self
    }

    /// Sets whether the window should start in fullscreen mode.
    pub fn fullscreen(mut self, fullscreen: bool) -> Self {
        self.0.fullscreen = fullscreen;
        self
    }

    /// Port number to start a server to listen to remote Firefox devtools connections. 0 for random port.
    pub fn devtools_port(mut self, port: u16) -> Self {
        self.0.devtools_port = Some(port);
        self
    }

    /// Sets the profiler settings.
    pub fn profiler_settings(mut self, settings: ProfilerSettings) -> Self {
        self.0.profiler_settings = Some(settings);
        self
    }

    /// Overrides the user agent.
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.0.user_agent = Some(user_agent.into());
        self
    }

    /// Sets the script to run when the document starts loading.
    // pub fn init_script(mut self, script: impl Into<String>) -> Self {
    //     self.0.init_script = Some(script.into());
    //     self
    // }

    /// Sets the directory to load user scripts from.
    pub fn userscripts_directory(mut self, directory: impl Into<String>) -> Self {
        self.0.userscripts_directory = Some(directory.into());
        self
    }

    /// Sets the initial zoom level of the webview.
    pub fn zoom_level(mut self, zoom: f32) -> Self {
        self.0.zoom_level = Some(zoom);
        self
    }

    /// Sets the resource directory path.
    pub fn resources_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.0.resources_directory = Some(path.into());
        self
    }

    /// Builds the `VersoviewController` with the configured settings.
    pub fn build(
        self,
        versoview_path: impl AsRef<Path>,
        initial_url: url::Url,
    ) -> VersoviewController {
        VersoviewController::create(versoview_path, initial_url, self.0)
    }
}
