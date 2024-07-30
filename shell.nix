with import <nixpkgs> { };
let
  nixgl = import (fetchTarball "https://github.com/nix-community/nixGL/archive/489d6b095ab9d289fe11af0219a9ff00fe87c7c5.tar.gz") { enable32bits = false; };
  pkgs_gnumake_4_3 = import (fetchTarball "https://github.com/NixOS/nixpkgs/archive/6adf48f53d819a7b6e15672817fa1e78e5f4e84f.tar.gz") { };
  llvmPackages = llvmPackages_14; # servo/servo#31059
  stdenv = stdenvAdapters.useMoldLinker llvmPackages.stdenv;
in
stdenv.mkDerivation {
  name = "verso-env";

  buildInputs = [
    fontconfig
    freetype
    libunwind
    xorg.libxcb
    xorg.libX11
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-bad
    gst_all_1.gst-plugins-ugly
    rustup
    taplo
    llvmPackages.bintools
    llvmPackages.llvm
    llvmPackages.libclang
    udev
    cmake
    dbus
    gcc
    git
    pkg-config
    which
    llvm
    perl
    yasm
    m4
    pkgs_gnumake_4_3.gnumake # servo/mozjs#375
    libGL
    mold
    wayland
    nixgl.auto.nixGLDefault
  ];
  LD_LIBRARY_PATH = lib.makeLibraryPath [
    zlib
    xorg.libXcursor
    xorg.libXrandr
    xorg.libXi
    libxkbcommon
    vulkan-loader
    wayland
    libGL
    nixgl.auto.nixGLDefault
  ];
  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  # Allow cargo to download crates
  SSL_CERT_FILE = "${cacert}/etc/ssl/certs/ca-bundle.crt";
  # Enable colored cargo and rustc output
  TERMINFO = "${ncurses.out}/share/terminfo";
}
