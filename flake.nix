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
    in {
      packages = forAllSystems (pkgs: rec {
        certirocq = (rocqPkgs pkgs).CertiRocq;        # 0.9.1+9.1, Rocq 9.1.1
        metarocq = (rocqPkgs pkgs).metarocq;          # 1.5.1-9.1
        compcert = (rocqPkgs pkgs).compcert;          # 3.17
        default = certirocq;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (rocqPkgs pkgs).coq
            (rocqPkgs pkgs).CertiRocq
          ];
        };
      });
    };
}
