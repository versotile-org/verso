use std::{collections::HashMap, env, fs::File, io::Read, path::PathBuf};

use getopts::Options;
use servo::{
    config::{basedir, opts, prefs},
    embedder_traits::resources::{self, Resource, ResourceReaderMethods},
};

/// Init options and preferences.
///
/// TODO write down how prefs and opts work.
pub fn init() {
    resources::set(Box::new(ResourceReader));
    // Reads opts first and then prefs.
    let args: Vec<String> = env::args().collect();
    let opts = Options::new();
    // FIXME: Most results are not handled. Better wait for user feedback to handle each config.
    let _ = opts::from_cmdline_args(opts, &args);

    let user_prefs_path = opts::get()
        .config_dir
        .clone()
        .or_else(basedir::default_config_dir)
        .map(|path| path.join("prefs.json"))
        .filter(|path| path.exists());

    let userprefs = if let Some(path) = user_prefs_path {
        let mut file = File::open(path).expect("Error opening user prefs");
        let mut txt = String::new();
        file.read_to_string(&mut txt)
            .expect("Can't read user prefs file");
        prefs::read_prefs_map(&txt).expect("Can't parse user prefs file")
    } else {
        HashMap::new()
    };

    prefs::add_user_prefs(userprefs);
}

struct ResourceReader;

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
            Resource::DirectoryListingHTML => {
                &include_bytes!("../resources/directory-listing.html")[..]
            }
        }
        .to_owned()
    }

    fn sandbox_access_files(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn sandbox_access_files_dirs(&self) -> Vec<PathBuf> {
        vec![]
    }
}
