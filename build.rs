use std::{env, fs::File, path::PathBuf};

use gl_generator::{Api, Fallbacks, Profile, Registry, StructGenerator};

fn main() {
    cfg_aliases::cfg_aliases! {
        // Platforms
        android: { target_os = "android" },
        macos: { target_os = "macos" },
        ios: { target_os = "ios" },
        // windows: { target_os = "windows" },
        apple: { any(target_os = "ios", target_os = "macos") },
        linux: { all(unix, not(apple), not(android)) },
    }

    #[cfg(all(feature = "packager", target_os = "macos"))]
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Resources/lib");

    // Generate GL bindings
    let dest = PathBuf::from(&env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=build.rs");
    let mut file = File::create(dest.join("gl_bindings.rs")).unwrap();
    Registry::new(Api::Gles2, (3, 0), Profile::Core, Fallbacks::All, [])
        .write_bindings(StructGenerator, &mut file)
        .unwrap();
}
