use std::{collections::HashMap, env, fs::File, io::Read};

use getopts::Options;
use servo::config::{basedir, opts, prefs};

pub fn init() {
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
