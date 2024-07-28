# Verso

[![project chat](https://img.shields.io/badge/zulip-57a7ff?style=for-the-badge&labelColor=555555&logo=zulip)](https://versotile.zulipchat.com/)

A web browser that plays old world blues to build new world hope.

![](https://github.com/pewsheen/verso/assets/460329/7df44c7d-a4c5-4393-8378-a8b7bc438b03)

Verso is a web browser built on top of Servo web engine. It's still under development. We dont' accept any feature request at the moment.
But if you are interested, feel free to help test it.

# Usage

## Prerequisites

### Windows

- Install [scoop](https://scoop.sh/) and then install other tools:

```sh
scoop install git python llvm cmake curl
```

> You can also use chocolatey to install if you prefer it.

### MacOS

- Install [Xcode](https://developer.apple.com/xcode/)
- Install [Homebrew](https://brew.sh/) and then install other tools:

```sh
brew install cmake pkg-config harfbuzz
```

### Linux

For unified environment setup and package experience, we choose Flatpak to build the project from the start.
Please follow [Flatpack Setup](https://flatpak.org/setup/) page to install Flakpak based on your distribution.
If you prefer to build the project without any sandbox, please follow the instructions in [Servo book](https://book.servo.org/hacking/setting-up-your-environment.html#tools-for-linux) to bootstrap.
But please understand we don't triage any build issue without flatpak setup.

#### Flatpak

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
flatpak-builder --user --install --force-clean target org.versotile.vero.yml
flatpak run org.versotile.verso
```

## Build

- Run demo

```sh
cargo run
```

## Nightly Release

Nightly releases built with CrabNebula Cloud can be found at [releases](https://web.crabnebula.cloud/verso/verso-nightly/releases).

## Future Work

- Multiwindow support.
- Enable multiprocess mode.
- Enable sandobx in all platforms.
- Enable `Gstreamer` feature and remove `brew install harfbuzz` in README.
