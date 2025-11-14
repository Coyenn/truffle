{
  description = "Truffle - A Rust CLI tool for managing 2D game assets";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = ["rust-src" "rustfmt" "clippy"];
        };

        buildInputs = with pkgs; [
          rustToolchain
          pkg-config
          # ImageMagick for highlight command
          imagemagick
        ];
      in {
        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          shellHook = ''
            echo "Truffle development environment"
            echo "Rust version: $(rustc --version)"
          '';
        };
      }
    );
}
