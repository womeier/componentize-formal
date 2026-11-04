#!/usr/bin/env python3
"""Assert the heavy things are fetched from a cache, never built locally.

flake.nix pins a nixpkgs revision chosen so CertiRocq's dependencies are the
exact store paths coq-community CI has already pushed to its cache. If that
stops being true -- an upstream cache garbage-collects, a pin drifts -- nothing
breaks loudly. `nix develop` just starts compiling MetaRocq, for hours, on
someone's laptop. This turns that into a failure in seconds.

CertiRocq itself is the deliberate exception: it is built from the `certirocq`
flake input rather than taken from nixpkgs, because the adapter needs the
function-table export no release has (TODO.md#table-export). So it is in no
cache by construction and is not checked here. What is checked is everything
underneath it -- MetaRocq is the multi-hour build, and it is still a hit.

`--max-jobs 0` forbids local building, so any path that must be built rather
than fetched fails the run. It cannot be pointed at a dev shell directly:
`nix develop` has to realise the shell's own wrapper derivation, which is
trivial, specific to this flake, and in no cache anywhere -- so the flag would
always fail, and for a reason that says nothing about the pin. The wrapper's
*inputs* are the interesting part, so those are what gets checked.

    python3 checks/substitutable.py
"""

from __future__ import annotations

import argparse
import subprocess
import sys

# Flake attributes whose closures must come from a cache. Not `certirocq`: see
# the module docstring -- it is built from a branch on purpose.
PACKAGES = ["metarocq", "compcert"]

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
            f"substitutable: `nix {' '.join(args)}` failed:\n{result.stderr}"
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
            f"substitutable: could not read the inputs of devShells.{system}."
            f"{name}:\n{refs.stderr}"
        )

    inputs = [f"{line}^*" for line in refs.stdout.split() if line.endswith(".drv")]
    if not inputs:
        raise SystemExit(
            f"substitutable: devShells.{system}.{name} reported no input "
            "derivations, so this check would pass without testing anything."
        )
    return inputs


def realise(what: str, installables: list[str]) -> bool:
    """Fetch these without building anything. True if that was possible."""
    print(f"substitutable: {what} ({len(installables)} derivation(s))", flush=True)
    result = subprocess.run(
        ["nix", "build", "--max-jobs", "0", "--no-link", *installables],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode == 0:
        return True

    print(f"\nsubstitutable: {what} is not fully cached.", file=sys.stderr)
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
            "see the comment on `inputs.nixpkgs` in flake.nix. CertiRocq itself\n"
            "is expected to be built locally and is not what this reports.",
            file=sys.stderr,
        )
        return 1

    print("substitutable: everything heavy is a cache hit")
    return 0


if __name__ == "__main__":
    sys.exit(main())
