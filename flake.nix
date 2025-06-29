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

        matrix-export-tool = craneLib.buildPackage {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;

          nativeBuildInputs = matrix-export-tool.buildInputs;
        };
      in
      {
        checks = {
          inherit matrix-export-tool;
        };

        packages.default = matrix-export-tool;

        devShells.default = craneLib.devShell {
          # Inherit inputs from checks.
          checks = self.checks.${system};

          # Extra inputs can be added here; cargo and rustc are provided by default
          # from the toolchain that was specified earlier.
          packages =
            [
            ];
        };
      }
    );
}
