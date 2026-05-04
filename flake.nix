{
  description = "Kermit – relational algebra research and benchmarking";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.git-cliff
            pkgs.cargo-expand
            pkgs.python313
          ];

          MIRIFLAGS = "-Zmiri-disable-isolation";
          RUST_BACKTRACE = "1";

          # Pip-installed Python wheels (numpy, matplotlib) are linked against
          # a glibc-style runtime loader and dlopen libstdc++.so.6 / libz.so.1
          # by bare name. On NixOS those libs aren't on the default search
          # path, so expose them here for any venv activated inside the shell.
          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [ pkgs.stdenv.cc.cc.lib pkgs.zlib ]}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
          '';
        };
      }
    );
}
