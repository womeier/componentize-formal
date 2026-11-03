#!/usr/bin/env python3
"""Layer 1 of representation conformance: hold repr.rs to what CertiRocq emits.

`bindgen/src/repr.rs` records the shape of the module the backend produces --
the export names the adapter links to, the function type indices it calls
through, that the module imports nothing and has a single memory. Its unit
tests pin those constants against the hand-written adapters, which catches a
mistake in repr.rs but not the backend moving underneath it.

So this compiles `checks/rocq/exports.v` with the real CertiRocq and reads the
facts back off the result. The expected names are read out of repr.rs rather
than restated here: a rename there must not leave this quietly checking the
names it used to have.

A drift caught here is not a crash. The adapter would load the wrong bytes and
the component would return plausible garbage, which is the failure mode this
repo exists to prevent -- so every mismatch is reported, not just the first,
and the exit status is non-zero.

Run it from a checkout inside `nix develop`, or via `nix flake check`:

    python3 checks/representation.py
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# Constants in repr.rs's `exports` module, each naming something the backend
# puts on the module. TABLE is the newest: see TODO.md#table-export.
BACKEND_EXPORTS = [
    "MEMORY",
    "MEM_PTR",
    "RESULT",
    "OUT_OF_MEM",
    "MAIN_FUNCTION",
    "TABLE",
]

# `CertiRocq Compile Wasm -file "conformance"` in the .v decides this.
MODULE_STEM = "conformance"

CONST = re.compile(r'^\s*pub const (?P<name>[A-Z_]+): &str = "(?P<value>[^"]*)";', re.M)
EXPORT = re.compile(r'\(export "(?P<name>[^"]*)"')
IMPORT = re.compile(r'^\s*\(import ', re.M)
MEMORY = re.compile(r'^\s*\(memory \(;\d+;\)', re.M)
FUNC_TYPE = re.compile(r'^\s*\(type \(;(?P<index>\d+);\) \(func(?P<rest>.*)\)\s*$', re.M)
PARAM = re.compile(r'\b(i32|i64|f32|f64|v128|funcref|externref)\b')


class Mismatch(Exception):
    """The inputs could not be read at all, so nothing can be concluded."""


def declared_names(repr_rs: Path) -> dict[str, str]:
    """The string each `pub const` in repr.rs is bound to."""
    text = repr_rs.read_text()
    names: dict[str, str] = {}
    for match in CONST.finditer(text):
        name, value = match.group("name"), match.group("value")
        if name in names:
            raise Mismatch(f"{repr_rs}: `pub const {name}` is defined twice")
        names[name] = value

    missing = [c for c in BACKEND_EXPORTS if c not in names]
    if missing:
        raise Mismatch(
            f"{repr_rs} no longer defines {', '.join(missing)}. "
            "This check cannot verify a constant it cannot find; update it "
            "alongside repr.rs."
        )
    return names


def rocq_filename(source: Path) -> str:
    """A copy name `rocq c` will accept.

    The filename becomes a Rocq module name, which may not contain `-` -- and
    a path handed over by Nix always does, its store hash being `<hash>-name`.
    """
    stem = re.sub(r"[^A-Za-z0-9_]", "_", source.stem)
    if not stem or stem[0].isdigit():
        stem = f"v_{stem}"
    return f"{stem}.v"


def compile_wasm(rocq_source: Path, workdir: Path) -> Path:
    """`rocq c` the file and return the .wasm the plugin wrote beside it."""
    local = workdir / rocq_filename(rocq_source)
    shutil.copy(rocq_source, local)

    env = dict(os.environ, HOME=str(workdir))
    result = subprocess.run(
        ["rocq", "c", local.name], cwd=workdir, env=env, text=True, capture_output=True
    )
    if result.returncode != 0:
        raise Mismatch(
            f"`rocq c {local.name}` failed with status {result.returncode}:\n"
            f"{result.stdout}{result.stderr}"
        )

    wasm = workdir / f"{MODULE_STEM}.wasm"
    if not wasm.is_file():
        produced = sorted(p.name for p in workdir.iterdir())
        raise Mismatch(
            f"`rocq c` produced no {wasm.name}; the -file name in "
            f"{rocq_source.name} and MODULE_STEM here must agree. "
            f"The build directory holds: {', '.join(produced)}"
        )
    return wasm


def disassemble(wasm: Path) -> str:
    subprocess.run(["wasm-tools", "validate", str(wasm)], check=True)
    return subprocess.run(
        ["wasm-tools", "parse", str(wasm), "-t"],
        check=True,
        text=True,
        capture_output=True,
    ).stdout


def check(names: dict[str, str], wat: str) -> list[str]:
    """Every way the module disagrees with repr.rs."""
    problems: list[str] = []

    exported = {m.group("name") for m in EXPORT.finditer(wat)}
    if not exported:
        raise Mismatch(
            "found no exports at all in the disassembly, so the pattern this "
            "check reads them with no longer matches wasm-tools' output"
        )

    for const in BACKEND_EXPORTS:
        if names[const] not in exported:
            # The table is the one that can be missing because the backend is
            # older than the assumption, rather than because it was renamed.
            if const == "TABLE":
                problems.append(
                    f'the backend does not export "{names[const]}", which the '
                    "adapter links the compiled closures through. A CertiRocq "
                    "predating that export cannot be used: move the nixpkgs "
                    "pin forward -- see TODO.md#table-export"
                )
            else:
                problems.append(
                    f'the backend no longer exports "{names[const]}" '
                    f"(repr.rs {const}); it exports {', '.join(sorted(exported))}"
                )

    # `mod_imports := []` is baked into the instantiation proof, and is what
    # the E-IMPORT rejection rests on.
    imports = len(IMPORT.findall(wat))
    if imports:
        problems.append(
            f"the emitted module now has {imports} import(s); both E-IMPORT's "
            "rationale and the proof's `mod_imports := []` assume none"
        )

    # One memory, so wasm-merge does not need --enable-multimemory.
    memories = len(MEMORY.findall(wat))
    if memories != 1:
        problems.append(f"expected exactly one memory, found {memories}")

    # repr::func_type_index(n) = n, i.e. type index n is the one taking n i32s.
    types = FUNC_TYPE.findall(wat)
    if len(types) < 2:
        raise Mismatch(
            f"found {len(types)} function types in the disassembly; the "
            "pattern this check reads them with no longer matches"
        )
    for index, rest in types:
        params = PARAM.findall(rest)
        if len(params) != int(index):
            problems.append(
                f"type index {index} takes {len(params)} parameters "
                f"({' '.join(params) or 'none'}); repr::func_type_index "
                f"assumes it takes {index}"
            )
        elif set(params) - {"i32"}:
            problems.append(
                f"type index {index} takes {' '.join(params)}; repr.rs assumes "
                "every Rocq function is [i32; n] -> []"
            )

    return problems


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--rocq-source",
        type=Path,
        default=root / "checks/rocq/exports.v",
        help="the .v to compile (default: %(default)s)",
    )
    parser.add_argument(
        "--repr",
        type=Path,
        default=root / "bindgen/src/repr.rs",
        help="the file whose constants are being held to account "
        "(default: %(default)s)",
    )
    args = parser.parse_args()

    try:
        names = declared_names(args.repr)
        with tempfile.TemporaryDirectory() as tmp:
            wasm = compile_wasm(args.rocq_source, Path(tmp))
            problems = check(names, disassemble(wasm))
    except Mismatch as e:
        print(f"representation: {e}", file=sys.stderr)
        return 1

    if problems:
        for problem in problems:
            print(f"representation: {problem}", file=sys.stderr)
        print(
            "\nA drift here does not make the adapter fail -- it makes it read "
            "the wrong bytes and return plausible garbage. See "
            "TODO.md#representation.",
            file=sys.stderr,
        )
        return 1

    checked = ", ".join(names[c] for c in BACKEND_EXPORTS)
    print(f"repr.rs agrees with CertiRocq: {checked}, no imports, one memory,")
    print("and type index n taking n i32s.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
