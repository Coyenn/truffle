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
        ];
      in {
        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          shellHook = ''
            echo "Truffle development environment"
            echo "Rust version: $(rustc --version)"
            echo "cargo: $(command -v cargo)"
            if command -v rustfmt >/dev/null 2>&1; then
              echo "rustfmt: $(command -v rustfmt) ($(rustfmt --version))"
            else
              echo "rustfmt: MISSING (expected via flake toolchain)"
            fi

            # If users still see rustup errors (e.g. missing 'cargo-fmt'), they're likely
            # *not* using this devShell. This hint makes the root cause obvious.
            case "$(command -v cargo 2>/dev/null || true)" in
              "$HOME"/.cargo/bin/*)
                echo "WARNING: cargo is from rustup (~/.cargo/bin). Run 'nix develop -c cargo fmt -- --check' or ensure direnv is loading this flake."
                ;;
            esac
          '';
        };
      }
    );
}
