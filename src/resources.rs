use std::{env, path::PathBuf};

use servo::embedder_traits::resources::{self, Resource, ResourceReaderMethods};

struct ResourceReader(PathBuf);

/// Initialize resource files. We currently read from `resources` directory only.
pub fn init() {
    resources::set(Box::new(ResourceReader(resources_dir_path())));
}

impl ResourceReaderMethods for ResourceReader {
    fn read(&self, file: Resource) -> Vec<u8> {
        match file {
            Resource::Preferences => &include_bytes!("../resources/prefs.json")[..],
            Resource::BluetoothBlocklist => &include_bytes!("../resources/gatt_blocklist.txt")[..],
            Resource::DomainList => &include_bytes!("../resources/public_domains.txt")[..],
            Resource::HstsPreloadList => &include_bytes!("../resources/hsts_preload.json")[..],
            Resource::BadCertHTML => &include_bytes!("../resources/badcert.html")[..],
            Resource::NetErrorHTML => &include_bytes!("../resources/neterror.html")[..],
            Resource::UserAgentCSS => &include_bytes!("../resources/user-agent.css")[..],
            Resource::ServoCSS => &include_bytes!("../resources/servo.css")[..],
            Resource::PresentationalHintsCSS => {
                &include_bytes!("../resources/presentational-hints.css")[..]
            }
            Resource::QuirksModeCSS => &include_bytes!("../resources/quirks-mode.css")[..],
            Resource::RippyPNG => &include_bytes!("../resources/rippy.png")[..],
            Resource::MediaControlsCSS => &include_bytes!("../resources/media-controls.css")[..],
            Resource::MediaControlsJS => &include_bytes!("../resources/media-controls.js")[..],
            Resource::CrashHTML => &include_bytes!("../resources/crash.html")[..],
        }
        .to_owned()
    }

    fn sandbox_access_files(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }

    fn sandbox_access_files_dirs(&self) -> Vec<PathBuf> {
        vec![]
    }
}

fn resources_dir_path() -> PathBuf {
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
