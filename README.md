# Verso

[![project chat](https://img.shields.io/badge/zulip-57a7ff?style=for-the-badge&labelColor=555555&logo=zulip)](https://versotile.zulipchat.com/)

A web browser that plays old world blues to build new world hope.

![](https://github.com/pewsheen/verso/assets/460329/7df44c7d-a4c5-4393-8378-a8b7bc438b03)

Verso is a web browser built on top of Servo web engine. It's still under development. We dont' accept any feature request at the moment.
But if you are interested, feel free to help test it.

# Usage

## Getting Started

### Windows

- Install [scoop](https://scoop.sh/) and then install other tools:

```sh
scoop install git python llvm cmake curl
python3 -m pip install mako
```

> You can also use chocolatey to install if you prefer it.

- Build & run:

```sh
cargo run
```

### MacOS

- Install [Xcode](https://developer.apple.com/xcode/)
- Install cmake and mako:

```sh
brew install cmake
python3 -m pip install mako
```

- Install current stable version of [Gstreamer](https://gstreamer.freedesktop.org/download/#macos). You need both "runtime" and "development" installers. After installation, you also need to set the `PATH` and `DYLD_FALLBACK_LIBRARY_PATH` environment variables as below. Note that the `pkg-config` from GStreamer should be the first one in the `PATH` as other versions have all kinds of quirks that will cause problems.

```sh
export PATH="/Library/Frameworks/GStreamer.framework/Versions/1.0/bin${PATH:+:$PATH}"
export DYLD_FALLBACK_LIBRARY_PATH="$DYLD_FALLBACK_LIBRARY_PATH:/Library/Frameworks/GStreamer.framework/Versions/1.0/lib"
```

- Build & run:

```sh
cargo run
```

### Linux

#### Flatpak

For unified environment setup and package experience, we choose Flatpak to build the project from the start.
Please follow [Flatpack Setup](https://flatpak.org/setup/) page to install Flakpak based on your distribution.

- Install flatpak runtimes and extensions:

```sh
flatpak install flathub org.freedesktop.Platform//23.08
flatpak install flathub org.freedesktop.Sdk//23.08
flatpak install flathub org.freedesktop.Sdk.Extension.rust-stable//23.08
flatpak install flathub org.freedesktop.Sdk.Extension.llvm18//23.08
```

- Generate manifests and build:
// TODO Exporting to a repository instead

```sh
python3 ./flatpak-cargo-generator.py ./Cargo.lock -o cargo-sources.json
flatpak-builder --user --install --force-clean target org.versotile.verso.yml
flatpak run org.versotile.verso
```

#### Nix

We also support building Verso in nix shell. But we don't bundle it in nix at the moment.

- For NixOS:

```sh
nix-shell shell.nix --run 'cargo r'
```

- For non-NixOS distributions:

```sh
nix-shell shell.nix --run 'nixGL cargo r'
```

If you prefer to build the project without any sandbox, please follow the instructions in [Servo book](https://book.servo.org/hacking/setting-up-your-environment.html#tools-for-linux) to bootstrap.
But please understand we don't triage any build issue without flatpak or nix setup.

## Nightly Release

Nightly releases built with CrabNebula Cloud can be found at [releases](https://web.crabnebula.cloud/verso/verso-nightly/releases).

## Future Work

- Multiwindow support.
- Enable multiprocess mode.
- Enable sandbox in all platforms.
- Enable `Gstreamer` media feature.
