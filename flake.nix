{
  description = "hodlcroft viewer development shell";

  inputs = {
    defrag-nix.url = "github:defrag-au/defrag-nix";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { defrag-nix, nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];
      mkShells =
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          base = defrag-nix.devShells.${system}.rust-worker-stack;

          # Rust toolchain with native target + WASM for the Leptos frontend
          # and the Cloudflare Worker. Tracks the same channel as the upstream
          # rust-worker-stack toolchain; bump together if needed.
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            targets = [
              "wasm32-unknown-unknown"
            ];
          };
        in
        {
          default = base.overrideAttrs (old: {
            # Prepend our rust toolchain so it shadows the upstream one in PATH.
            # just added so contributors don't need a global install to run
            # justfile recipes (e.g. `just sync-cardano`).
            nativeBuildInputs =
              [ rustToolchain ]
              ++ (old.nativeBuildInputs or [ ])
              ++ [
                pkgs.just
              ];
          });
        };
    in
    {
      devShells = builtins.listToAttrs (
        map (system: {
          name = system;
          value = mkShells system;
        }) systems
      );
    };
}
