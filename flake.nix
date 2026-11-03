{
  description = "componentize-formal";

  inputs = {
    # Pinned to the exact nixpkgs revision that coq-nix-toolbox pins
    # (.nix/nixpkgs.nix at toolbox rev 39f7d7aa, which CertiRocq's own CI uses),
    # so CertiRocq and its heavy dependencies (MetaRocq, CompCert) are the exact
    # store paths that coq-community CI has already pushed to
    # coq-community.cachix.org -- nothing has to be built locally.
    # Do not bump this to nixpkgs-unstable: any later revision rebuilds MetaRocq.
    nixpkgs.url = "github:NixOS/nixpkgs/fb9c9907f7655e040a5fedc979c80d386d7e7813";
  };

  # Substituters holding the prebuilt Rocq package set for the pin above.
  nixConfig = {
    extra-substituters = [
      "https://coq-community.cachix.org"
      "https://coq.cachix.org"
      "https://metarocq.cachix.org"
    ];
    extra-trusted-public-keys = [
      "coq-community.cachix.org-1:WBDHojv8FM6nI4ZMh43X+2g6j4WpAn+dFhjhWmLCgnA="
      "coq.cachix.org-1:5QW/wwEnD+l2jvN6QRbRRsa4hBHG3QiQQ26cxu1F5tI="
      "metarocq.cachix.org-1:lxAsIEhIjsuVfPVdMQ4maAcYbQNUO9MsJQvxlozoZY0="
    ];
  };

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;

      # No x86_64-darwin: nixpkgs 26.11, which the pin above is, dropped it, so
      # claiming it here only produces an evaluation error on an Intel Mac.
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];

      # CertiRocq pulls in CompCert, whose license (inria-compcert) is unfree in
      # nixpkgs, so it has to be allowed explicitly.
      pkgsFor = system: import nixpkgs {
        inherit system;
        config.allowUnfreePredicate = pkg: lib.getName pkg == "compcert";
      };

      forAllSystems = f: lib.genAttrs systems (system: f (pkgsFor system));

      # At this pin the Rocq package set still lives under `coqPackages`
      # (`rocqPackages` there has no CertiRocq attribute).
      rocqPkgs = pkgs: pkgs.coqPackages;

      # The component toolchain. wasm-tools does the embed/new steps and
      # binaryen supplies wasm-merge; both come from the pin above, so they are
      # cache hits rather than builds. bindgen is pinned against the wit-parser
      # version matching this wasm-tools, so WIT parsing and canonical-ABI
      # layout are literally the same code the component assembly uses --
      # `checks.abi-version` holds that pinning to its word.
      componentTools = pkgs: [
        pkgs.wasm-tools   # 1.254.0
        pkgs.binaryen     # 129, for wasm-merge
        pkgs.wasmtime     # 47.0.2
      ];

      rustTools = pkgs: [
        pkgs.cargo        # 1.97.0
        pkgs.rustc
        pkgs.clippy
        pkgs.rustfmt
      ];

      # The checks under checks/ are Python, and meant to be runnable by hand.
      scriptTools = pkgs: [ pkgs.python3 ];

      # TODO.md comes along because the test suite asserts that every rejection
      # code the gate can emit has a section there. Paths are listed explicitly
      # so the build doesn't rebuild on unrelated changes.
      bindgenSrc = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          ./bindgen/Cargo.toml
          ./bindgen/Cargo.lock
          ./bindgen/src
          ./bindgen/tests
          ./bindgen/wit          # the shipped certirocq:abi package
          ./TODO.md
        ];
      };

      bindgenFor = pkgs: pkgs.rustPlatform.buildRustPackage {
        pname = "componentize-formal";
        version = "0.1.0";

        src = bindgenSrc;
        sourceRoot = "source/bindgen";

        cargoLock.lockFile = ./bindgen/Cargo.lock;
        meta.description =
          "Turn a WIT world into a Wasm component backed by CertiRocq's verified Wasm backend";
      };
    in {
      packages = forAllSystems (pkgs: rec {
        certirocq = (rocqPkgs pkgs).CertiRocq;        # 0.9.1+9.1, Rocq 9.1.1
        metarocq = (rocqPkgs pkgs).metarocq;          # 1.5.1-9.1
        compcert = (rocqPkgs pkgs).compcert;          # 3.17

        componentize-formal = bindgenFor pkgs;

        default = certirocq;
      });

      # Everything CI enforces lives here, so `nix flake check` locally and CI
      # run the same thing.
      checks = forAllSystems (pkgs: let
        bindgen = bindgenFor pkgs;
      in {
        # Building the package runs the 13 integration and 7 unit tests in the
        # sandbox, which is stricter than `cargo test` in a checkout: it has
        # caught files missing from `bindgenSrc` that the checkout still had.
        componentize-formal = bindgen;

        clippy = bindgen.overrideAttrs (old: {
          pname = "${old.pname}-clippy";
          nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.clippy ];
          buildPhase = "cargo clippy --all-targets -- --deny warnings";
          doCheck = false;
          installPhase = "touch $out";
        });

        # `cargo fmt` rather than bare `rustfmt` so the edition comes from
        # Cargo.toml instead of being restated here.
        fmt = pkgs.runCommand "componentize-formal-fmt"
          { nativeBuildInputs = [ pkgs.cargo pkgs.rustfmt ]; } ''
            cp -r ${bindgenSrc}/bindgen crate
            chmod -R u+w crate
            export CARGO_HOME=$PWD/.cargo
            cd crate
            cargo fmt --check
            touch $out
          '';

        # Both scripts run standalone in `nix develop` too, and say what
        # they concluded rather than only whether they passed.
        abi-version = pkgs.runCommand "abi-version-match"
          { nativeBuildInputs = [ pkgs.python3 pkgs.wasm-tools ]; } ''
            python3 ${./checks/abi_version.py} --lock ${./bindgen/Cargo.lock}
            touch $out
          '';

        # Layer 1 of representation conformance (TODO.md#representation):
        # compile a dull function with the real CertiRocq and read back every
        # claim in repr.rs that is visible without running the module. repr.rs's
        # own unit tests cannot catch the backend moving underneath them.
        representation = pkgs.runCommand "representation-conformance"
          {
            nativeBuildInputs = [
              (rocqPkgs pkgs).coq
              (rocqPkgs pkgs).CertiRocq
              pkgs.python3
              pkgs.wasm-tools
            ];
          } ''
            python3 ${./checks/representation.py} \
              --rocq-source ${./checks/rocq/exports.v} \
              --repr ${./bindgen/src/repr.rs}
            touch $out
          '';
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (rocqPkgs pkgs).coq
            (rocqPkgs pkgs).CertiRocq
            pkgs.rust-analyzer
          ] ++ rustTools pkgs ++ componentTools pkgs ++ scriptTools pkgs;
        };

        # The same shell without the Rocq package set, for working on the
        # generator alone -- CertiRocq is a long substitution to wait on when
        # nothing you are about to run needs it.
        bindgen = pkgs.mkShell {
          packages = [ pkgs.rust-analyzer ]
            ++ rustTools pkgs ++ componentTools pkgs ++ scriptTools pkgs;
        };
      });
    };
}
