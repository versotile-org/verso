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
}
