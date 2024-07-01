fn main() {
    cfg_aliases::cfg_aliases! {
        // Platforms
        android: { target_os = "android" },
        macos: { target_os = "macos" },
        ios: { target_os = "ios" },
        // windows: { target_os = "windows" },
        apple: { any(target_os = "ios", target_os = "macos") },
        linux: { all(unix, not(apple), not(android)) },
        // Backends
        gtk: { all(feature = "native", linux) },
        gtk: { all(feature = "os-webview", linux) },
        servo: { all(feature = "servo", any(linux, macos, windows)) },
    }
}
