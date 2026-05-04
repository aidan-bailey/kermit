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
          ];

          MIRIFLAGS = "-Zmiri-disable-isolation";
          RUST_BACKTRACE = "1";

          # The vendored watdiv binary is dynamically linked against
          # libstdc++, which is not on a default search path on NixOS.
          # Expose the gcc lib output so the binary can load when invoked
          # by tests under `nix develop`. (Inherited into the bwrap
          # namespace because bwrap propagates env by default.)
          LD_LIBRARY_PATH = "${pkgs.stdenv.cc.cc.lib}/lib";
        };
      }
    );
}
