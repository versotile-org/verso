# Verso

[![project chat](https://img.shields.io/badge/zulip-57a7ff?style=for-the-badge&labelColor=555555&logo=zulip)](https://versotile.zulipchat.com/)

A web browser that plays old world blues to build new world hope.

<img src="https://github.com/user-attachments/assets/ca124b2b-c54c-4796-b2cb-0819228495b6" width="600" />

Verso is a web browser built on top of the [Servo](https://servo.org/) web engine. We aim to explore embedding solutions for Servo while growing it into a mature browser one day.
This means we want to experiment with multi-view and multi-window first and then build UI elements entirely from Servo itself. At the moment, [Servoshell](https://servo.org/download/) should provide a better user experience.

Verso is still under development. We don't accept feature requests at the moment, and the whole navigation workflow hasn't been polished yet, either. But if you are interested, feel free to open bug-fix PRs.

# Usage

## Getting Started

### Windows

- Install [scoop](https://scoop.sh/) and then install other tools:

```sh
scoop install git python llvm cmake curl
pip install mako
```

> You can also use chocolatey to install if you prefer it.

- Build & run:

```sh
cargo run
```

### MacOS

- Install [Xcode](https://developer.apple.com/xcode/)
- Install [Homebrew](https://brew.sh/) and then install other tools:

```sh
brew install cmake pkg-config harfbuzz
pip install mako
```

- Build & run:

```sh
cargo run
```

### Linux

#### Flatpak

For unified environment setup and package experience, we choose Flatpak to build the project from the start.
Please follow the [Flatpak Setup](https://flatpak.org/setup/) page to install Flatpak based on your distribution.

- Install flatpak runtimes and extensions:

```sh
flatpak install flathub org.freedesktop.Platform//24.08
flatpak install flathub org.freedesktop.Sdk//24.08
flatpak install flathub org.freedesktop.Sdk.Extension.rust-stable//24.08
flatpak install flathub org.freedesktop.Sdk.Extension.llvm18//24.08
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

> Packages are unsigned currently. If you have problem opening the app on macOS, try `xattr -d com.apple.quarantine /Applications/verso.app` after installation.

## Future Work

- Multi-window support.
- Enable multiprocess mode.
- Enable sandbox in all platforms.
- Enable `Gstreamer` feature.
