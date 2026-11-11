#!/usr/bin/env python3
"""Assert the heavy things are fetched from a cache, never built locally.

flake.nix pins a nixpkgs revision chosen so CertiRocq's dependencies are the
exact store paths coq-community CI has already pushed to its cache. If that
stops being true -- an upstream cache garbage-collects, a pin drifts -- nothing
breaks loudly. `nix develop` just starts compiling Rocq and its libraries, for
hours, on someone's laptop, on top of the hours this tree already asks for on
purpose. This turns that into a failure in seconds.

Three things are built locally on purpose, and this check gave up ground to
each of them (TODO.md#table-export): CertiRocq, because the adapter needs a
function-table export no release has; WasmCert-Coq, because CertiRocq `main`
needs 2.2.1 and the pin carries 2.2.0; and MetaRocq, because `main` imports a
module that exists only on MetaRocq's `9.1` branch. The last one hurts --
MetaRocq is the multi-hour build this check was written for, and no upstream
cache has it built against this pin.

So what is checked is what is left: CompCert, and the *inputs* MetaRocq is
built from -- Rocq itself, Equations, the standard library, the OCaml
toolchain. Those are what a drifted pin would silently start rebuilding, and
they are still hits. It is a weaker guarantee than before and worth
restoring: an upstream CertiRocq that exports the table would retire all
three exceptions at once.

What is asked of *upstream* is also only asked of upstream. flake.nix lists a
fourth substituter, `womeier.cachix.org`, which is this project's own and which
CI pushes to. It pushes closures, so an upstream path can land in it by being a
dependency of something that was built -- CompCert is one, under CertiRocq.
Left in, it would answer for the pin: the check would go green on our own
year-old copy long after the coq-community one was collected, which is the
exact failure it exists to catch. So the substituters are named here instead of
inherited, and `--no-accept-flake-config` stops flake.nix from appending the
project cache back on.

`--max-jobs 0` forbids local building, so any path that must be built rather
than fetched fails the run. It cannot be pointed at a dev shell directly:
`nix develop` has to realise the shell's own wrapper derivation, which is
trivial, specific to this flake, and in no cache anywhere -- so the flag would
always fail, and for a reason that says nothing about the pin. The wrapper's
*inputs* are the interesting part, so those are what gets checked.

    python3 checks/cache_hits.py
"""

from __future__ import annotations

import argparse
import subprocess
import sys

# The caches this check is about, and the keys for them. Stated here rather
# than taken from flake.nix's `nixConfig`, so that adding a substituter there
# cannot quietly change what this concludes -- see the module docstring on
# `womeier.cachix.org`. Passed with `--no-accept-flake-config`, which also
# means the keys have to come along: refusing the flake's config refuses its
# `extra-trusted-public-keys` too.
UPSTREAM_SUBSTITUTERS = " ".join([
    "https://cache.nixos.org",
    "https://coq-community.cachix.org",
    "https://coq.cachix.org",
    "https://metarocq.cachix.org",
])

UPSTREAM_KEYS = " ".join([
    "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=",
    "coq-community.cachix.org-1:WBDHojv8FM6nI4ZMh43X+2g6j4WpAn+dFhjhWmLCgnA=",
    "coq.cachix.org-1:5QW/wwEnD+l2jvN6QRbRRsa4hBHG3QiQQ26cxu1F5tI=",
    "metarocq.cachix.org-1:lxAsIEhIjsuVfPVdMQ4maAcYbQNUO9MsJQvxlozoZY0=",
])

# Flake attributes whose whole closure must come from a cache. Not `certirocq`
# and no longer `metarocq`: see the module docstring -- both are built here on
# purpose.
PACKAGES = ["compcert"]

# Flake attributes that are built locally, but whose inputs must still be
# hits. This is what is left of the MetaRocq guarantee: the pin can no longer
# be checked by fetching MetaRocq, so it is checked by fetching everything
# MetaRocq is compiled against.
#
# `metarocq.utils`, not `metarocq`: the meta-package's inputs are the other
# ten MetaRocq derivations, which are local builds too, so asking about them
# only re-reports the exception. `utils` sits at the bottom of the chain and
# depends on no other MetaRocq package, so its inputs are exactly the
# toolchain underneath -- Rocq, Equations, the standard library, ExtLib,
# zarith -- which is the thing a drifted pin would rebuild.
LOCAL_PACKAGES = ["metarocq.utils"]

# Dev shells whose dependencies must come from a cache. The shells themselves
# are excluded, per the module docstring. `default` is excluded too, because
# it carries the locally-built CertiRocq; `bindgen` is the shell this check
# can speak for, and the packages above cover what `default` adds.
SHELLS = ["bindgen"]


def nix(*args: str) -> str:
    result = subprocess.run(
        ["nix", *args], text=True, capture_output=True, check=False
    )
    if result.returncode != 0:
        raise SystemExit(
            f"cache-hits: `nix {' '.join(args)}` failed:\n{result.stderr}"
        )
    return result.stdout.strip()


def current_system() -> str:
    return nix("eval", "--impure", "--raw", "--expr", "builtins.currentSystem")


def shell_inputs(system: str, name: str) -> list[str]:
    """The derivations a dev shell depends on, as `<path>^*` install.

    Asking for the shell itself would ask for the wrapper; its direct input
    derivations are the toolchain it puts on PATH.
    """
    drv = nix("eval", "--raw", f".#devShells.{system}.{name}.drvPath")
    refs = subprocess.run(
        ["nix-store", "--query", "--references", drv],
        text=True,
        capture_output=True,
        check=False,
    )
    if refs.returncode != 0:
        raise SystemExit(
            f"cache-hits: could not read the inputs of devShells.{system}."
            f"{name}:\n{refs.stderr}"
        )

    inputs = [f"{line}^*" for line in refs.stdout.split() if line.endswith(".drv")]
    if not inputs:
        raise SystemExit(
            f"cache-hits: devShells.{system}.{name} reported no input "
            "derivations, so this check would pass without testing anything."
        )
    return inputs


def package_inputs(attr: str) -> list[str]:
    """The derivations a locally-built package is compiled against.

    Its own sources are dropped: they are this repo's pins -- a CertiRocq
    revision, a MetaRocq revision -- so they are in no cache by construction
    and would fail the run for a reason that says nothing about nixpkgs.
    """
    drv = nix("eval", "--raw", f".#{attr}.drvPath")
    refs = subprocess.run(
        ["nix-store", "--query", "--references", drv],
        text=True,
        capture_output=True,
        check=False,
    )
    if refs.returncode != 0:
        raise SystemExit(
            f"cache-hits: could not read the inputs of {attr}:\n{refs.stderr}"
        )

    inputs = [
        f"{line}^*"
        for line in refs.stdout.split()
        if line.endswith(".drv") and not line.endswith("-source.drv")
    ]
    if not inputs:
        raise SystemExit(
            f"cache-hits: {attr} reported no input derivations, so this "
            "check would pass without testing anything."
        )
    return inputs


def realise(what: str, installables: list[str]) -> bool:
    """Fetch these without building anything. True if that was possible."""
    print(f"cache-hits: {what} ({len(installables)} derivation(s))", flush=True)
    result = subprocess.run(
        [
            "nix", "build", "--max-jobs", "0", "--no-link",
            "--no-accept-flake-config",
            "--option", "substituters", UPSTREAM_SUBSTITUTERS,
            "--option", "trusted-public-keys", UPSTREAM_KEYS,
            *installables,
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode == 0:
        return True

    print(f"\ncache-hits: {what} is not fully cached.", file=sys.stderr)
    print(result.stderr.strip(), file=sys.stderr)
    return False


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--system",
        default=None,
        help="flake system to check (default: this machine's)",
    )
    args = parser.parse_args()
    system = args.system or current_system()

    ok = realise(
        "the Rocq package set", [f".#{name}" for name in PACKAGES]
    )
    for attr in LOCAL_PACKAGES:
        ok &= realise(f"what {attr} is built against", package_inputs(attr))
    for shell in SHELLS:
        ok &= realise(
            f"the inputs of devShells.{system}.{shell}",
            shell_inputs(system, shell),
        )

    if not ok:
        print(
            "\nSomething that should be a cache hit would be built locally.\n"
            "If an upstream cache was garbage-collected, either rebuild and push\n"
            "it, or move the nixpkgs pin to a revision that is still cached --\n"
            "see the comment on `inputs.nixpkgs` in flake.nix. CertiRocq,\n"
            "MetaRocq and WasmCert-Coq are expected to be built locally and are\n"
            "not what this reports, and neither does womeier.cachix.org count\n"
            "here -- this asks upstream only.",
            file=sys.stderr,
        )
        return 1

    print("cache-hits: everything heavy is a cache hit")
    return 0


if __name__ == "__main__":
    sys.exit(main())
