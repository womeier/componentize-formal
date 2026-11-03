{
  description = "CertiRocq components";

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
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      # CertiRocq pulls in CompCert, whose license (inria-compcert) is unfree in
      # nixpkgs, so it has to be allowed explicitly.
      pkgsFor = system: import nixpkgs {
        inherit system;
        config.allowUnfreePredicate = pkg: nixpkgs.lib.getName pkg == "compcert";
      };

      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (pkgsFor system));

      # At this pin the Rocq package set still lives under `coqPackages`
      # (`rocqPackages` there has no CertiRocq attribute).
      rocqPkgs = pkgs: pkgs.coqPackages;

      # The component toolchain. wasm-tools does the embed/new steps and
      # binaryen supplies wasm-merge; both come from the pin above, so they are
      # cache hits rather than builds. bindgen is pinned against the wit-parser
      # version matching this wasm-tools, so WIT parsing and canonical-ABI
      # layout are literally the same code the component assembly uses.
      componentTools = pkgs: [
        pkgs.wasm-tools   # 1.254.0
        pkgs.binaryen     # 129, for wasm-merge
        pkgs.wasmtime     # 47.0.2
      ];
    in {
      packages = forAllSystems (pkgs: rec {
        certirocq = (rocqPkgs pkgs).CertiRocq;        # 0.9.1+9.1, Rocq 9.1.1
        metarocq = (rocqPkgs pkgs).metarocq;          # 1.5.1-9.1
        compcert = (rocqPkgs pkgs).compcert;          # 3.17

        bindgen = pkgs.rustPlatform.buildRustPackage {
          pname = "rocq-component-bindgen";
          version = "0.1.0";

          # TODO.md comes along because the test suite asserts that every
          # rejection code the gate can emit has a section there. Paths are
          # listed explicitly so the build doesn't rebuild on unrelated changes.
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./bindgen/Cargo.toml
              ./bindgen/Cargo.lock
              ./bindgen/src
              ./bindgen/tests
              ./bindgen/wit          # the shipped certirocq:abi package
              ./TODO.md
            ];
          };
          sourceRoot = "source/bindgen";

          cargoLock.lockFile = ./bindgen/Cargo.lock;
          meta.description =
            "Generate Rocq interfaces and canonical-ABI glue for CertiRocq's Wasm backend from a WIT world";
        };

        default = certirocq;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (rocqPkgs pkgs).coq
            (rocqPkgs pkgs).CertiRocq
            pkgs.cargo        # 1.97.0
            pkgs.rustc
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
          ] ++ componentTools pkgs;
        };
      });
    };
}
