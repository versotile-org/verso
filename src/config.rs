use std::{fs, path::PathBuf};

use embedder_traits::resources::{self, Resource, ResourceReaderMethods};
use servo_config::opts::{default_opts, set_options, Opts};

/// Configuration of Verso instance.
#[derive(Clone, Debug)]
pub struct Config {
    /// Global flag options of Servo.
    pub opts: Opts,
    /// Path to resources directory.
    pub resource_dir: PathBuf,
}

impl Config {
    /// Create a new configuration for creating Verso instance. It must provide the path of
    /// resources directory.
    pub fn new(resource_dir: PathBuf) -> Self {
        let opts = default_opts();
        Self { opts, resource_dir }
    }

    /// Init options and preferences.
    pub fn init(self) {
        // Set the resource files and preferences of Servo.
        resources::set(Box::new(ResourceReader(self.resource_dir)));

        // Set the global options of Servo.
        set_options(self.opts);
    }
}

impl Config {
    /// Returns the path to the resources directory.
    pub fn resources_dir_path() -> Option<std::path::PathBuf> {
        #[cfg(feature = "packager")]
        let root_dir = {
            use cargo_packager_resource_resolver::{current_format, resources_dir};
            current_format().and_then(|format| resources_dir(format))
        };
        #[cfg(not(feature = "packager"))]
        let root_dir = std::env::current_dir();

        root_dir.ok().map(|dir| dir.join("resources"))
    }
}

struct ResourceReader(PathBuf);

impl ResourceReaderMethods for ResourceReader {
    fn read(&self, file: Resource) -> Vec<u8> {
        let path = self.0.join(file.filename());
        fs::read(path).expect("Can't read file")
    }

    fn sandbox_access_files(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn sandbox_access_files_dirs(&self) -> Vec<PathBuf> {
        vec![]
    }
}
