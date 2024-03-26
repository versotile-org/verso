use std::{env, fs, path::PathBuf};

use servo::embedder_traits::resources::{self, Resource, ResourceReaderMethods};

struct ResourceReader(PathBuf);

#[cfg(not(debug_assertions))]
use cargo_packager_resource_resolver::{current_format, resources_dir};

/// Initialize resource files. We currently read from `resources` directory only.
pub fn init() {
    resources::set(Box::new(ResourceReader(resources_dir_path())));
}

impl ResourceReaderMethods for ResourceReader {
    fn read(&self, res: Resource) -> Vec<u8> {
        let mut path = self.0.clone();
        path.push(res.filename());
        fs::read(path).expect("Can't read file")
    }

    fn sandbox_access_files(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }

    fn sandbox_access_files_dirs(&self) -> Vec<PathBuf> {
        vec![]
    }
}

fn resources_dir_path() -> PathBuf {
    // For production builds, use Resourse Resolver
    #[cfg(not(debug_assertions))]
    return resources_dir(current_format().unwrap())
        .unwrap()
        .join("resources");

    // Try ./resources relative to the directory containing the
    // canonicalised executable path, then each of its ancestors.
    let mut path = env::current_exe().unwrap().canonicalize().unwrap();
    while path.pop() {
        path.push("resources");
        if path.is_dir() {
            return path;
        }
        path.pop();
    }
    panic!("Can not find the resources directory. Please specify it in WebContext instead.");
}
