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
```

- Build & run:

```sh
cargo run
```

### Linux


#### Nix

- For NixOS:

```sh
nix-shell shell.nix --run 'cargo r'
```

- For non-NixOS distributions:

```sh
nix-shell shell.nix --run 'nixGL cargo r'
```

#### Debian-based Distributions

```sh
sudo apt install build-essential python3-pip ccache clang cmake curl \
g++ git gperf libdbus-1-dev libfreetype6-dev libgl1-mesa-dri \
libgles2-mesa-dev libglib2.0-dev libgstreamer-plugins-base1.0-dev \
gstreamer1.0-plugins-good libgstreamer-plugins-good1.0-dev \
gstreamer1.0-plugins-bad libgstreamer-plugins-bad1.0-dev \
gstreamer1.0-plugins-ugly gstreamer1.0-plugins-base \
libgstreamer-plugins-base1.0-dev gstreamer1.0-libav \
libgstrtspserver-1.0-dev gstreamer1.0-tools libges-1.0-dev \
libharfbuzz-dev liblzma-dev libunwind-dev libunwind-dev libvulkan1 \
libx11-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
libxmu-dev libxmu6 libegl1-mesa-dev llvm-dev m4 xorg-dev
```

For others, please follow the instructions in [Servo book](https://book.servo.org/hacking/setting-up-your-environment.html#tools-for-linux) to bootstrap first.

## Nightly Release

Nightly releases built with CrabNebula Cloud can be found at [releases](https://web.crabnebula.cloud/verso/verso-nightly/releases).

## Future Work

- Multiwindow support.
- Enable multiprocess mode.
- Enable sandobx in all platforms.
- Enable `Gstreamer` feature and remove `brew install harfbuzz` in README.
