{
  description = "CLI utility for exporting matrix chat history";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane";

    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      flake-utils,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain (
          p:
          p.rust-bin.nightly.latest.default.override {
            targets = [ "x86_64-unknown-linux-gnu" ];

            extensions = [
              "rust-analyzer"
              "rust-src"
            ];
          }
        );

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;

          nativeBuildInputs = with pkgs; [
            pkg-config
            mold
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly (
          commonArgs
          // {
            pname = "matrix-export-tool-deps";
          }
        );

        matrix-export-tool-clippy = craneLib.cargoClippy (
          commonArgs
          // {
            inherit cargoArtifacts;
            # cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            cargoClippyExtraArgs = "--all-targets";
          }
        );

        matrix-export-tool-coverage = craneLib.cargoTarpaulin (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );

        matrix-export-tool = craneLib.buildPackage (
          commonArgs
          // rec {
            inherit cargoArtifacts;
            dontPatchELF = true;

            buildInputs =
              with pkgs;
              [
                fontconfig
                freetype
              ]
              ++ lib.optionals stdenv.hostPlatform.isLinux [
                alsa-lib
                libGL
                vulkan-loader
                wayland
                libx11
                libxcb
                libxext
                libxkbcommon
              ];

            # Note for non-NixOS: This still requires using `nixVulkanIntel` from the nixGL repo.
            # I have spent way too much time being confused over this quirk.
            postFixup =
              with pkgs;
              lib.optionalString stdenv.hostPlatform.isLinux ''
                patchelf --add-rpath ${
                  lib.makeLibraryPath [
                    wayland
                    vulkan-loader
                    libglvnd
                  ]
                } $out/bin/matrix-export-tool
              '';
          }
        );

      in
      {
        packages.default = matrix-export-tool;

        checks = {
          inherit
            matrix-export-tool
            matrix-export-tool-clippy
            matrix-export-tool-coverage
            ;
        };

        devShells.default = craneLib.devShell {
          # Inherit inputs from checks.
          checks = self.checks.${system};

          # Extra inputs can be added here; cargo and rustc are provided by default
          # from the toolchain that was specified earlier.
          packages = with pkgs; [
            cargo-edit
          ];
        };
      }
    );
}
