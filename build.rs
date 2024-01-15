// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

fn main() {
  let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
  if target_os == "macos" || target_os == "ios" {
    println!("cargo:rustc-link-lib=framework=WebKit");
  }

  let is_android = std::env::var("CARGO_CFG_TARGET_OS")
    .map(|t| t == "android")
    .unwrap_or_default();
  if is_android {
    use std::{fs, path::PathBuf};

    fn env_var(var: &str) -> String {
      std::env::var(var).unwrap_or_else(|_| {
        panic!(
          "`{}` is not set, which is needed to generate the kotlin files for android.",
          var
        )
      })
    }

    println!("cargo:rerun-if-env-changed=WRY_ANDROID_PACKAGE");
    println!("cargo:rerun-if-env-changed=WRY_ANDROID_LIBRARY");
    println!("cargo:rerun-if-env-changed=WRY_ANDROID_KOTLIN_FILES_OUT_DIR");

    if let Ok(kotlin_out_dir) = std::env::var("WRY_ANDROID_KOTLIN_FILES_OUT_DIR") {
      let package = env_var("WRY_ANDROID_PACKAGE");
      let library = env_var("WRY_ANDROID_LIBRARY");

      let kotlin_out_dir = PathBuf::from(&kotlin_out_dir)
        .canonicalize()
        .unwrap_or_else(move |_| {
          panic!("Failed to canonicalize `WRY_ANDROID_KOTLIN_FILES_OUT_DIR` path {kotlin_out_dir}")
        });

      let kotlin_files_path =
        PathBuf::from(env_var("CARGO_MANIFEST_DIR")).join("src/android/kotlin");
      println!("cargo:rerun-if-changed={}", kotlin_files_path.display());
      let kotlin_files = fs::read_dir(kotlin_files_path).expect("failed to read kotlin directory");

      for file in kotlin_files {
        let file = file.unwrap();

        let class_extension_env = format!(
          "WRY_{}_CLASS_EXTENSION",
          file
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_uppercase()
        );
        let class_init_env = format!(
          "WRY_{}_CLASS_INIT",
          file
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_uppercase()
        );

        println!("cargo:rerun-if-env-changed={}", class_extension_env);
        println!("cargo:rerun-if-env-changed={}", class_init_env);

        let content = fs::read_to_string(file.path())
          .expect("failed to read kotlin file as string")
          .replace("{{package}}", &package)
          .replace("{{library}}", &library)
          .replace(
            "{{class-extension}}",
            &std::env::var(&class_extension_env).unwrap_or_default(),
          )
          .replace(
            "{{class-init}}",
            &std::env::var(&class_init_env).unwrap_or_default(),
          );

        let auto_generated_comment = match file
          .path()
          .extension()
          .unwrap_or_default()
          .to_str()
          .unwrap_or_default()
        {
          "pro" => "# THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!!\n\n",
          "kt" => "/* THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!! */\n\n",
          _ => "String::new()",
        };
        let mut out = String::from(auto_generated_comment);
        out.push_str(&content);

        let out_path = kotlin_out_dir.join(file.file_name());
        fs::write(&out_path, out).expect("Failed to write kotlin file");
        println!("cargo:rerun-if-changed={}", out_path.display());
      }
    }
  }

  cfg_aliases::cfg_aliases! {
      // Platforms
      android: { target_os = "android" },
      macos: { target_os = "macos" },
      ios: { target_os = "ios" },
      windows: { target_os = "windows" },
      apple: { any(target_os = "ios", target_os = "macos") },
      linux: { all(unix, not(apple), not(android)) },
      // Backends
      gtk: { all(feature = "native", linux) },
      gtk: { all(feature = "os-webview", linux) },
      servo: { all(feature = "servo", any(linux, macos, windows)) },
  }
}
