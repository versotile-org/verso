{
  description = "Verso - A web browser that plays old world blues to build new world hope.";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";

    nixpkgs.url = "github:nixos/nixpkgs/release-24.05";
    nixpkgs_gnumake_4_3.url = "github:nixos/nixpkgs?rev=6adf48f53d819a7b6e15672817fa1e78e5f4e84f";

    nixgl.url = "github:nix-community/nixGL?rev=489d6b095ab9d289fe11af0219a9ff00fe87c7c5";
    nixgl.inputs.nixpkgs.follows = "nixpkgs";

    cargo2nix.url = "github:cargo2nix/cargo2nix/release-0.11.0";
    cargo2nix.inputs.nixpkgs.follows = "nixpkgs";
    cargo2nix.inputs.flake-utils.follows = "flake-utils";
  };

  outputs = { self, cargo2nix, flake-utils, nixpkgs, nixpkgs_gnumake_4_3, ... } @inputs:
    let
      cargo_toml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
    in
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs
          {
            inherit system;
            config = { allowUnfree = true; };
            overlays = [
              # FIXME: infinite recursion
              # (final: prev:
              #   let
              #     nixgl = import inputs.nixgl {
              #       pkgs = prev;
              #       enable32bits = false;
              #     };
              #   in
              #   nixgl.overlay final prev)
              inputs.nixgl.overlay
              cargo2nix.overlays.default
            ];
          };

        pkgs_gnumake_4_3 = import nixpkgs_gnumake_4_3 {
          inherit system;
        };

        rustPkgs = pkgs.rustBuilder.makePackageSet {
          rustVersion = "1.75.0";
          packageFun = import ./Cargo.nix;
        };

        llvmPackages = pkgs.llvmPackages_14;

        mkShell = pkgs.mkShell.override {
          stdenv = pkgs.stdenvAdapters.useMoldLinker pkgs.llvmPackages_14.stdenv;
        };

        python = (pkgs.python3.withPackages (ps: with ps; [ pip pydbus mako ]));
      in
      {
        devShells = rec {
          default = dev;

          dev = mkShell {
            buildInputs = with pkgs; [
              fontconfig
              freetype
              libunwind
              xorg.libxcb
              xorg.libX11
              gst_all_1.gstreamer
              gst_all_1.gst-plugins-base
              gst_all_1.gst-plugins-bad
              gst_all_1.gst-plugins-ugly
              cargo
              rustc
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
              python

              # nix
              nil
              nixpkgs-fmt
            ];

            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
              zlib
              xorg.libXcursor
              xorg.libXrandr
              xorg.libXi
              libxkbcommon
              vulkan-loader
              wayland
              libGL
              nixgl.auto.nixGLDefault
            ]);

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

            # Allow cargo to download crates
            SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";

            # Enable colored cargo and rustc output
            TERMINFO = "${pkgs.ncurses.out}/share/terminfo";
          };
        };

        packages = rec {
          default = verso-nixpkgs;

          verso-cargo2nix = (rustPkgs.workspace.verso { });

          verso-nixpkgs = pkgs.rustPlatform.buildRustPackage {
            name = cargo_toml.package.name;
            version = cargo_toml.package.version;
            src = self;
            cargoHash = "";
            buildInputs = with pkgs; [
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
            meta = {
              description = cargo_toml.package.description;
              homepage = cargo_toml.package.homepage;
              license = pkgs.lib.licenses.asl20;
            };
          };
        };
      }
    );
}
