use std::{fs, path::PathBuf};

use embedder_traits::resources::{self, Resource, ResourceReaderMethods};
use headers::{ContentType, HeaderMapExt};
use net::protocols::{ProtocolHandler, ProtocolRegistry};
use net_traits::{
    request::Request,
    response::{Response, ResponseBody},
    ResourceFetchTiming,
};
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

    /// Register URL scheme protocols
    pub fn create_protocols(&self) -> ProtocolRegistry {
        let handler = ResourceReader(self.resource_dir.clone());
        let mut protocols = ProtocolRegistry::with_internal_protocols();
        protocols.register("verso", handler);
        protocols
    }

    /// Init options and preferences.
    pub fn init(self) {
        // Set the resource files and preferences of Servo.
        resources::set(Box::new(ResourceReader(self.resource_dir)));

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

impl ProtocolHandler for ResourceReader {
    fn load(
        &self,
        request: &mut Request,
        _done_chan: &mut net::fetch::methods::DoneChannel,
        _context: &net::fetch::methods::FetchContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
        let path = self.0.join(request.current_url().domain().unwrap());

        let response = if let Ok(file) = fs::read(path) {
            let mut response = Response::new(
                request.current_url(),
                ResourceFetchTiming::new(request.timing_type()),
            );

            // Set Content-Type header.
            // TODO: We assume it's HTML for now. This should be updated once we have IPC interface.
            response.headers.typed_insert(ContentType::html());

            *response.body.lock().unwrap() = ResponseBody::Done(file);

            response
        } else {
            Response::network_internal_error("Opening file failed")
        };

        Box::pin(std::future::ready(response))
    }
}
