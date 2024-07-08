use std::{fs, path::PathBuf};

use servo::{
    config::opts::{default_opts, set_options, Opts},
    embedder_traits::resources::{self, Resource, ResourceReaderMethods},
};

/// Configuration of Verso instance.
#[derive(Clone, Debug)]
pub struct Config {
    /// Global flag options of Servo.
    pub opts: Opts,
}

impl Config {
    /// Create a new configuration for creating Verso instance. It must provide the path of
    /// resources directory.
    pub fn new(path: PathBuf) -> Self {
        let mut opts = default_opts();
        opts.config_dir = Some(path);
        Self { opts }
    }

    /// Init options and preferences.
    ///
    /// TODO write down how prefs and opts work.
    pub fn init(self) {
        // Set the resource files and preferences of Servo.
        let config_dir = self
            .opts
            .config_dir
            .clone()
            .filter(|d| d.exists())
            .expect("Can't get the resources directory.");
        resources::set(Box::new(ResourceReader(config_dir)));

        // Set the global options of Servo.
        set_options(self.opts);
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
