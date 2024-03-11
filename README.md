# Verso

A web browser that plays old world blues to build new world hope.

https://github.com/wusyong/Yippee/assets/8409985/a7a92fa4-5980-44d1-a9b5-81ff23c01ba6

# Usage

The current demo works best on macOS at the moment, since it tries to customize its traffic light buttons to be seamless in the window.

However, We plan to focus on Windows as main target support.

## Prerequisites

### Windows

- Install [scoop](https://scoop.sh/) and then install other tools:

```sh
scoop install git python llvm cmake curl
```

### MacOS

- Install [Xcode](https://developer.apple.com/xcode/)
- Install [Homebrew](https://brew.sh/) and then install other tools:

```sh
brew install cmake pkg-config
```

### Linux

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

For others, please follow the instructions in [Servo's wiki](https://github.com/servo/servo/wiki/Building) to bootstrap first.

## Build

- Run demo

```sh
cargo run
```

- Or if you are using Nix or NixOS, add `wayland` and `libGL` to `LD_LIBRARY_PATH` in `../servo/etc/shell.nix`

```
nix-shell ../servo/etc/shell.nix --run 'cargo run'
```

## Future Work

- Add more window and servo features to make it feel more like a general web browser.
- Improve  development experience.
- Multi webviews and multi browsing contexts in the same window.
