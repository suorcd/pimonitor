{
  description = "Pimonitor development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
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
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Use the latest stable Rust toolchain
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
          ];
        };

        # Build-time dependencies (e.g., compilers, config tools)
        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        # Runtime/Link-time dependencies
        buildInputs =
          with pkgs;
          [
            # Add common dependencies here if any
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            # Linux specific dependencies for rodio/cpal
            alsa-lib
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            # macOS specific frameworks for rodio/cpal and reqwest/security
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.CoreFoundation
            darwin.apple_sdk.frameworks.CoreAudio
            darwin.apple_sdk.frameworks.AudioUnit
          ];

      in
      {
        devShells.default = pkgs.mkShell {
          inherit buildInputs nativeBuildInputs;

          packages = [
            rustToolchain
          ];

          # Set environment variables if needed
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      }
    );
}
