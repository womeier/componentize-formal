#!/usr/bin/env python3
"""Hold the wit-parser pin to the wasm-tools the toolchain actually ships.

The generator computes canonical-ABI layout with wit-parser; `wasm-tools
component new` computes the layout the host reads. They agree only because
they are the same code at the same version -- wit-parser 0.N.x is the library
inside wasm-tools 1.N.x.

Left unchecked this fails silently and expensively: the component builds, the
host reads fields at offsets the adapter never wrote, and `wasmtime` hands back
plausible garbage. So a mismatch fails the build instead.

    python3 checks/abi_version.py
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

# [[package]]
# name = "wit-parser"
# version = "0.254.2"
LOCKED = re.compile(
    r'^name = "wit-parser"\nversion = "(?P<major>\d+)\.(?P<minor>\d+)\.',
    re.M,
)
REPORTED = re.compile(r"^wasm-tools (?P<major>\d+)\.(?P<minor>\d+)\.")


def locked_minor(lock: Path) -> int:
    found = LOCKED.findall(lock.read_text())
    if len(found) != 1:
        raise SystemExit(
            f"abi-version: expected exactly one wit-parser entry in {lock}, "
            f"found {len(found)}. Without it this check cannot conclude "
            "anything, so it fails rather than pass vacuously."
        )
    major, minor = found[0]
    if major != "0":
        raise SystemExit(
            f"abi-version: wit-parser is at {major}.{minor}, past the 0.x line "
            "this check's version correspondence assumes. Re-establish how its "
            "version maps to wasm-tools', then update this check."
        )
    return int(minor)


def shipped_minor() -> int:
    try:
        reported = subprocess.run(
            ["wasm-tools", "--version"], check=True, text=True, capture_output=True
        ).stdout
    except (OSError, subprocess.CalledProcessError) as e:
        raise SystemExit(f"abi-version: could not run `wasm-tools --version`: {e}")

    match = REPORTED.match(reported)
    if not match:
        raise SystemExit(
            "abi-version: could not read a version out of "
            f"`wasm-tools --version`, which printed: {reported.strip()!r}"
        )
    return int(match.group("minor"))


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--lock",
        type=Path,
        default=root / "bindgen/Cargo.lock",
        help="the lockfile pinning wit-parser (default: %(default)s)",
    )
    args = parser.parse_args()

    locked, shipped = locked_minor(args.lock), shipped_minor()
    if locked != shipped:
        print(
            f"abi-version: wit-parser 0.{locked}.x does not match wasm-tools "
            f"1.{shipped}.x.\n"
            "The generator would compute a different canonical-ABI layout than "
            "the component tooling, and the mismatch would surface as a working "
            f"component returning wrong values. Pin bindgen/Cargo.toml to "
            f"wit-parser 0.{shipped}, or pin nixpkgs back.",
            file=sys.stderr,
        )
        return 1

    print(f"wit-parser 0.{locked}.x matches wasm-tools 1.{shipped}.x")
    return 0


if __name__ == "__main__":
    sys.exit(main())
