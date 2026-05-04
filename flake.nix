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
            # bubblewrap sandboxes the vendored watdiv binary; required by
            # `kermit bench watdiv-gen` and the kermit-rdf e2e test.
            pkgs.bubblewrap
            pkgs.python313
          ];

          MIRIFLAGS = "-Zmiri-disable-isolation";
          RUST_BACKTRACE = "1";

          # LD_LIBRARY_PATH covers two NixOS-specific dynamic-loading needs:
          #   - the vendored watdiv binary (kermit-rdf/vendor/watdiv) dlopens
          #     libstdc++.so.6 by bare name — required by the kermit-rdf e2e
          #     test and `kermit bench watdiv-gen`. Inherited into the bwrap
          #     namespace because bwrap propagates env by default.
          #   - pip-installed Python wheels (numpy, matplotlib) used by
          #     scripts/kermit-plot/ dlopen libstdc++.so.6 / libz.so.1.
          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [ pkgs.stdenv.cc.cc.lib pkgs.zlib ]}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
          '';
        };
      }
    );
}
