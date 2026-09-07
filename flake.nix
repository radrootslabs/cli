{
  description = "Radroots command-line interface";

  inputs = {
    crane.url = "github:ipetkov/crane/01bc1d404a51a0a07e9d8759cd50a7903e218c82";
    lib = {
      url = "github:radrootslabs/lib/3563f3b5a4331eb2cb3f925cafc9de524d844228";
      inputs.crane.follows = "crane";
    };
    nixpkgs.follows = "lib/nixpkgs";
    rust-overlay.follows = "lib/rust-overlay";
  };

  outputs =
    {
      crane,
      lib,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-linux"
      ];
      forAllSystems =
        function:
        builtins.listToAttrs (
          map (system: {
            name = system;
            value = function system;
          }) systems
        );
      cliOutputs =
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          source = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              craneLib.filterCargoSources path type
              || pkgs.lib.hasSuffix ".json" (baseNameOf path)
              || pkgs.lib.hasSuffix ".txt" (baseNameOf path)
              || baseNameOf path == "flake.nix"
              || baseNameOf path == "README";
            name = "radroots-cli-source";
          };
          commonArgs = {
            src = source;
            cargoLock = ./Cargo.lock;
            strictDeps = true;
            doCheck = false;
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          package = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              pname = "radroots_cli";
              version = "0.1.0";
              CARGO_PROFILE = "release";
              cargoExtraArgs = "--locked --package radroots_cli --bin radroots";
            }
          );
          check = craneLib.mkCargoDerivation (
            commonArgs
            // {
              inherit cargoArtifacts;
              pname = "radroots-cli-check";
              version = "1";
              buildPhaseCargoCommand = "cargo check --locked --package radroots_cli --all-targets";
              installPhaseCommand = "mkdir -p $out";
            }
          );
          app = {
            type = "app";
            program = "${package}/bin/radroots";
            meta.description = "Run the built radroots CLI";
          };
        in
        {
          inherit app check package;
        };
    in
    {
      packages = forAllSystems (system: {
        default = (cliOutputs system).package;
      });
      checks = forAllSystems (system: {
        default = (cliOutputs system).check;
      });
      apps = forAllSystems (system: {
        default = (cliOutputs system).app;
      });
    };
}
