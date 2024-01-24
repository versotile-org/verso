# Yippee

A web browser.

https://github.com/wusyong/Yippee/assets/8409985/a7a92fa4-5980-44d1-a9b5-81ff23c01ba6

# Usage

The current demo works best on macOS at the moment, since it tries to customize its traffic light buttons to be seamless in the window.

It should also work on Windows, as well as Linux with X11. You may encounter problems running the demo on Linux with Wayland or Xwayland.

## Prerequisites

### Windows

- Install [Rust](https://www.rust-lang.org/)
- Install [scoop](https://scoop.sh/) and then install other tools from it. You can install them manually but this is easier and recommended:

```sh
scoop install git python llvm cmake curl
```

### Others(WIP)

For now, please follow the instructions in [Servo - Build Setup](https://github.com/servo/servo) to bootstrap first.

## Build

- **NixOS only:** add `wayland` and `libGL` to `LD_LIBRARY_PATH` in `../servo/etc/shell.nix`

- Run demo

```sh
cargo run
```

  - Or if you are using Nix or NixOS:

  ```
  nix-shell ../servo/etc/shell.nix --run 'cargo run --example servo'
  ```

## Future Work

- Add more window and servo features to make it feel more like a general web browser.
- Improve  development experience.
- Multi webviews and multi browsing contexts in the same window.
