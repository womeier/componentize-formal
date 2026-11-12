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

    # CertiRocq itself comes from here rather than from nixpkgs, because the
    # adapter needs the generated module to export its function table and no
    # CertiRocq release exports it -- nor does `main`, whose export list at
    # `LambdaANF_to_Wasm.v:568` still ends at `MED_mem`. See
    # TODO.md#table-export.
    #
    # `womeier/certirocq` at `wasm-export-table`, pinned to its head
    # `546dd8a8`: upstream `main` (`943b52ae`) plus one commit, the eleven
    # lines that add the export to `LambdaANF_to_Wasm.v` and discharge the
    # extra `ET_table` obligation the instantiation proof then carries in
    # `LambdaANF_to_Wasm_instantiation.v`. Nothing is admitted around, and
    # `rocq c` re-checks the obligation on every build, so a rebase that
    # invalidates the proof fails here loudly rather than silently dropping
    # it.
    #
    # Pinned by revision, not by `wasm-export-table`: a branch pin would
    # follow the next rebase on its own, which is how a tree that once
    # compiled stops compiling with nothing in this file having changed.
    #
    # Rebased onto `main` rather than onto a release, because the release is
    # 349 commits back and carries the old WasmCert and MetaRocq bounds with
    # it. That is not free: `main` needs two dependencies newer than the
    # nixpkgs pin has, so `rocqPkgs` below moves both and both become local
    # builds -- WasmCert-Coq 2.2.1, which is short, and MetaRocq at the head
    # of its `9.1` branch, which is hours. The comments there carry the
    # evidence for each. Only CompCert and the rest of the Rocq set are still
    # the cache hits `checks/cache_hits.py` speaks for, and that check says
    # what it can no longer cover.
    certirocq = {
      url = "github:womeier/certirocq/546dd8a89f6fe15756b5b67176650bc8222e4ad4";
      flake = false;
    };
  };

  nixConfig = {
    extra-substituters = [
      "https://womeier.cachix.org"
      "https://coq-community.cachix.org"
      "https://coq.cachix.org"
      "https://metarocq.cachix.org"
    ];
    extra-trusted-public-keys = [
      "womeier.cachix.org-1:0R8+V2ZDOk7mKAA+hF2VulnUoM3Qh8jmeDkcV8gw2Rs="
      "coq-community.cachix.org-1:WBDHojv8FM6nI4ZMh43X+2g6j4WpAn+dFhjhWmLCgnA="
      "coq.cachix.org-1:5QW/wwEnD+l2jvN6QRbRRsa4hBHG3QiQQ26cxu1F5tI="
      "metarocq.cachix.org-1:lxAsIEhIjsuVfPVdMQ4maAcYbQNUO9MsJQvxlozoZY0="
    ];
  };

  outputs = { self, nixpkgs, certirocq }:
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
      #
      # MetaRocq is moved to the head of its `9.1` branch, and it is the
      # expensive one. CertiRocq `main` imports
      # `MetaRocq.Erasure.EImplementLazyForce`
      # (`theories/Compiler/metarocq_pipeline.v:19`,
      # `theories/Compiler/correctness.v:36`), and that module exists in no
      # MetaRocq release: the pinned 1.5.1-9.1 package set carries
      # `EImplementBox` and nothing else, in any of its outputs. CertiRocq's
      # own `.nix/config.nix` says the same thing -- its only bundle sets
      # `metarocq.override.version = "9.1"` -- and `main` carries a commit
      # named "Update to latest MetaRocq 9.1 branch", so the branch head is
      # the version `main` is written against, not a version it tolerates.
      #
      # Pinned by revision rather than by the branch name upstream CI uses:
      # `version = "9.1"` makes meta-fetch take the `isString` path, which
      # fetches the branch archive with no hash at all. That evaluates only
      # with `--impure` and silently follows the branch, so two runs a day
      # apart would compile different MetaRocqs under the same store name.
      #
      # The branch head satisfies everything the pin provides -- its opam
      # wants `rocq-core >= 9.1.0 & < 9.2` (9.1.1), `rocq-equations >= 1.3.1`
      # (1.3.1+9.1), `rocq-stdlib` 9.x and `coq-ext-lib >= 0.12` (0.13.1) --
      # so, like WasmCert, it needs no patching to build here.
      #
      # WasmCert-Coq is moved for a narrower reason, and cheaply. `main`'s
      # `rocq-certirocq.opam:40` asks for `coq-wasm >= 2.2.1` and that is not
      # a soft bound: `CodegenWasm/LambdaANF_to_Wasm_utils.v:1173` uses
      # `write_bytes_gen_preserve_length`, which 2.2.1 adds in
      # `Wasm/type_preservation.v:678` and 2.2.0 does not have anywhere. Built
      # against the pin's 2.2.0 the compile stops exactly there, with
      # `The variable write_bytes_gen_preserve_length was not found in the
      # current environment`.
      #
      # 2.2.1 itself needs nothing newer than the pin already has -- its opam
      # wants `coq-compcert >= 3.14` (the pin has 3.17, which is also the
      # version CertiRocq pins exactly), `coq-mathcomp-ssreflect < 2.7~`
      # (2.5.0), flocq 4.2.2, ext-lib 0.13.1 and ocaml `wasm` 2.0.2 -- so it
      # compiles against this dependency set unpatched. Overriding in the
      # scope rather than at the call site is what makes CertiRocq's
      # `propagatedBuildInputs` name the same WasmCert its modules were
      # compiled against; a `.override` on CertiRocq alone would leave 2.2.0
      # propagated into every consumer's load path.
      #
      # The pin's `wasmcert` expression has `release` entries for 2.1.0 and
      # 2.2.0 only, so 2.2.1 arrives as the attribute form `meta-fetch`
      # accepts, supplying the tag and hash the missing entry would have held.
      rocqPkgs = pkgs: pkgs.coqPackages.overrideScope (final: prev: {
        metarocq = prev.metarocq.override {
          version = {
            version = "9.1-51b14e4b";
            location = {
              rev = "51b14e4bb5f4580f00f60208ff8dbf31f3ad6f56";
              hash = "sha256-sQ4q/ndbWiiXg4xhT3mNfPz1qjOi3hTu5wfLm27lIjQ=";
            };
          };
        };

        wasmcert = prev.wasmcert.override {
          version = {
            version = "2.2.1";
            location = {
              rev = "v2.2.1";
              hash = "sha256-RtJXfaX/04+BuMWjnq6PKOyNm5Ed5v/ZP71C9Q8iSVk=";
            };
          };
        };
      });

      # CertiRocq from the `certirocq` input with the table export patched in,
      # built against the dependency set above: the build inputs, the Rocq
      # version and the install layout are whatever the nixpkgs expression
      # decided, and only the source moves.
      #
      # The source goes in through `version`, as the attribute form whose
      # `fetcher` hands back the flake input, rather than through
      # `overrideAttrs` on `src`. Two reasons. The nixpkgs expression derives
      # its `defaultVersion` from `metarocq-erasure-plugin.version` and knows
      # only `1.5.1-9.1`, so with MetaRocq moved to the branch head that
      # lookup falls off the end and yields `broken` -- which `overrideAttrs`
      # on `src` alone cannot undo, and the package then refuses to evaluate.
      # Passing a `version` skips the lookup. And `mkCoqDerivation` builds
      # `name` from the version it is given, so the store path reads
      # `coq9.1-CertiRocq-wasm-export-table-546dd8a8`: a module that exports
      # the table never shares a name with a released one that does not, which
      # is the confusion this repo exists to avoid.
      #
      # The phases need two corrections on top of that, both because the
      # nixpkgs expression describes the release's tree and `main`'s is
      # different. They are not guesses: CertiRocq carries its own package in
      # `.nix/coq-overlays/CertiRocq/default.nix`, which is what its CI
      # builds `main` with, and this is where the two differ.
      #
      #   * `python3`. `main`'s top-level Makefile generates the two plugin
      #     manifests with `plugins/manifests/generate.py`, and the rule fires
      #     during the build because it depends on
      #     `theories/Extraction/extraction.vo`, which the build has just
      #     made. The release had no such step, so the nixpkgs expression
      #     brings only clang and `make plugins` would die on `python3: not
      #     found` after the whole of `theories` had compiled.
      #   * the plugin directories. `main` moved them to `plugins/plugin` and
      #     `plugins/cplugin`; the nixpkgs `installPhase` still installs from
      #     `plugin` and `cplugin`, which do not exist, so it fails there --
      #     after an hour of compiling, at the last step.
      #
      # `installPhase` is written out rather than rewritten from the old one,
      # so the next layout change is a build failure naming the directory it
      # cannot find, not a quiet install of fewer files than intended.
      #
      # Deliberately *not* copied from that overlay: `coqprime`, which it
      # propagates and nothing under `theories` or `libraries` imports at this
      # revision, and the `metarocq` meta-package, where the erasure and
      # safechecker plugins the nixpkgs expression already names are enough --
      # nothing imports `MetaRocq.Quotation` or `MetaRocq.Translations`. Both
      # would only add build time; if `main` starts needing them, it says so
      # by failing.
      certirocqFor = pkgs:
        let
          scope = rocqPkgs pkgs;
        in
        (scope.CertiRocq.override {
          version = {
            version = "wasm-export-table-546dd8a8";
            fetcher = _: certirocq;
          };
        }).overrideAttrs (old: {
          nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.python3 ];

          installPhase = ''
            runHook preInstall

            OUTDIR=$out/lib/coq/${scope.coq.coq-version}/user-contrib

            DST=$OUTDIR/CertiRocq/Plugin/runtime make -C runtime install
            COQLIBINSTALL=$OUTDIR make -C theories install
            COQLIBINSTALL=$OUTDIR make -C libraries install
            COQLIBINSTALL=$OUTDIR COQPLUGININSTALL=$OCAMLFIND_DESTDIR \
              make -C plugins/plugin install
            COQLIBINSTALL=$OUTDIR COQPLUGININSTALL=$OCAMLFIND_DESTDIR \
              make -C plugins/cplugin install

            runHook postInstall
          '';
        });

      # The component toolchain. wasm-tools assembles the component from the
      # generated text and comes from the pin above, so it is a cache hit
      # rather than a build. bindgen is pinned against the wit-parser
      # version matching this wasm-tools, so WIT parsing and canonical-ABI
      # layout are literally the same code the component assembly uses --
      # `checks.abi-version` holds that pinning to its word.
      componentTools = pkgs: [
        pkgs.wasm-tools   # 1.254.0
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
        certirocq = certirocqFor pkgs;               # main + the table export (546dd8a8)
        metarocq = (rocqPkgs pkgs).metarocq;          # 9.1 branch at 51b14e4b
        compcert = (rocqPkgs pkgs).compcert;          # 3.17

        componentize-formal = bindgenFor pkgs;

        default = certirocq;
      });

      # Everything CI enforces lives here, so `nix flake check` locally and CI
      # run the same thing.
      checks = forAllSystems (pkgs: let
        bindgen = bindgenFor pkgs;
      in {
        # Building the package runs the 31 integration and 8 unit tests in the
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
              (certirocqFor pkgs)
              pkgs.python3
              pkgs.wasm-tools
            ];
          } ''
            python3 ${./checks/representation.py} \
              --rocq-source ${./checks/rocq/exports.v} \
              --constructor-source ${./checks/rocq/constructors.v} \
              --measured ${./bindgen/src/measured.rs} \
              --repr ${./bindgen/src/repr.rs}
            touch $out
          '';

        # Layer 2 of the same thing: the module has to *run*. One generated
        # world per representation per position, compiled with the real
        # CertiRocq, merged with its generated adapter and invoked -- which is
        # the one step the stub path cannot take, because a stub is written
        # against the same repr.rs the adapter is and agrees with it by
        # construction. Also the only place the out-of-memory channel fires on
        # a real allocator rather than a poisoned bump pointer.
        #
        # `${./checks}/backend.py` rather than `${./checks/backend.py}`
        # because it imports the representation list out of roundtrip.py,
        # which has to be beside it in the store.
        backend = pkgs.runCommand "backend-behaviour"
          {
            nativeBuildInputs = [
              (rocqPkgs pkgs).coq
              (certirocqFor pkgs)
              pkgs.python3
              pkgs.gnumake
              pkgs.wasm-tools
              pkgs.wasmtime
            ];
          } ''
            python3 ${./checks}/backend.py \
              --generator ${bindgen}/bin/componentize-formal \
              --cases ${./checks}/backend \
              --plan ${./bindgen/src/emit/plan.rs}
            touch $out
          '';

        # The generator against the hand-written examples: same code, then the
        # same answers out of a component assembled from what it wrote. The
        # examples are never regenerated -- they were written and run before
        # the emitter existed, which is what makes them a reference. For the
        # ones written ahead of the gate it checks the refusal instead: the
        # right rejection code, and nothing written.
        #
        # `${./checks}/generated.py` rather than `${./checks/generated.py}`
        # because it imports the expectations from `smoke.py`, which has to be
        # beside it in the store.
        generated = pkgs.runCommand "generated"
          {
            nativeBuildInputs = [
              pkgs.python3
              pkgs.gnumake
              pkgs.wasm-tools
              pkgs.wasmtime
            ];
          } ''
            python3 ${./checks}/generated.py \
              --generator ${bindgen}/bin/componentize-formal \
              --examples ${./examples} \
              --fixtures ${./bindgen/tests/wit/good}
            touch $out
          '';

        # The two conversions in emit/convert.rs, composed both ways and run.
        # An export adapter uses each direction in one position only; the
        # other order is what a function import will need
        # (TODO.md#imports), and nothing in the repository runs it yet. This
        # does, over every world in the good fixture set.
        roundtrip = pkgs.runCommand "roundtrip"
          {
            nativeBuildInputs = [
              pkgs.python3
              pkgs.wasmtime
            ];
          } ''
            python3 ${./checks/roundtrip.py} \
              --generator ${bindgen}/bin/componentize-formal \
              --fixtures ${./bindgen/tests/wit/good} \
              --plan ${./bindgen/src/emit/plan.rs}
            touch $out
          '';

        # Assembles each example into a component and checks what it answers.
        # Needs no Rocq: the examples link against `stub/`, which stands in for
        # the backend's output, so this runs on every push while
        # `checks.representation` is what holds the stub to the real thing.
        smoke = pkgs.runCommand "smoke"
          {
            nativeBuildInputs = [
              pkgs.python3
              pkgs.gnumake
              pkgs.wasm-tools
              pkgs.wasmtime
            ];
          } ''
            python3 ${./checks/smoke.py} --examples ${./examples}
            touch $out
          '';
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (rocqPkgs pkgs).coq
            (certirocqFor pkgs)
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
