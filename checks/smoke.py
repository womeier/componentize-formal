#!/usr/bin/env python3
"""Assemble every example into a component and check what it answers.

The examples are hand-written in the shape the generator has to emit, so this
is the only end-to-end evidence in the repo that the canonical-ABI glue is
right: `wasm-merge`, `component embed`, `component new`, then `wasmtime` with
the answers spelled out below.

Each expectation is written out in full rather than computed, because a
computed expectation can be wrong in the same way the adapter is. Three of
them are load-bearing:

  * `plus1(9223372036854775807) = ok(0)` -- Uint63 wraps, so wrapping is the
    right answer and the adapter must not invent a failure there.
  * `plus1(9223372036854775808) = err(out-of-range)` -- that u64 has no
    Uint63.int, and the hand-written reference adapter in CertiRocq's
    testing/examples returns a wrong answer here instead.
  * `err(out-of-memory)` -- the allocator is reported, not trapped on.

Runs against `stub/`, which stands in for CertiRocq's output, so it needs no
Rocq and no nixpkgs pin -- just wasm-tools, binaryen and wasmtime.

    python3 checks/smoke.py
"""

from __future__ import annotations

import argparse
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

# example -> make target -> the exact lines it must print.
EXPECTED: dict[str, dict[str, list[str]]] = {
    "plus1": {
        "run-stub": [
            "plus1(0) = ok(1)",
            "plus1(8) = ok(9)",
            "plus1(4611686018427387903) = ok(4611686018427387904)",
            "plus1(9223372036854775807) = ok(0)",
            "plus1(9223372036854775808) = err(out-of-range)",
        ],
        "run-stub-oom": [
            "plus1(8) with the heap exhausted = err(out-of-memory)",
        ],
    },
    "negb": {
        "run-stub": [
            "negb(true) = ok(false)",
            "negb(false) = ok(true)",
        ],
        "run-stub-oom": [
            "negb(true) with the heap exhausted = err(out-of-memory)",
        ],
    },
}


def run_target(example: Path, target: str) -> list[str]:
    """`make -s <target>` in a scratch copy, so the checkout stays clean."""
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp) / example.name
        shutil.copytree(example, work)
        # The source may be a read-only Nix store path, and the build writes
        # next to the sources.
        for path in [work, *work.rglob("*")]:
            path.chmod(path.stat().st_mode | stat.S_IWUSR)
        result = subprocess.run(
            ["make", "-s", target],
            cwd=work,
            text=True,
            capture_output=True,
            check=False,
        )
    if result.returncode != 0:
        raise SystemExit(
            f"smoke: `make {target}` in {example.name} failed with status "
            f"{result.returncode}:\n{result.stdout}{result.stderr}"
        )
    return [line.rstrip() for line in result.stdout.splitlines() if line.strip()]


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--examples",
        type=Path,
        default=root / "examples",
        help="directory holding the examples (default: %(default)s)",
    )
    args = parser.parse_args()

    on_disk = {p.name for p in args.examples.iterdir() if p.is_dir()}
    if on_disk != set(EXPECTED):
        raise SystemExit(
            f"smoke: {args.examples} holds {sorted(on_disk)} but this check "
            f"knows {sorted(EXPECTED)}. An example nothing runs is an example "
            "that rots; add it here."
        )

    problems = []
    for name, targets in EXPECTED.items():
        for target, want in targets.items():
            got = run_target(args.examples / name, target)
            if got == want:
                print(f"smoke: {name} {target} ({len(want)} answer(s))")
                continue
            problems.append(f"{name} {target}:")
            for line in want:
                if line not in got:
                    problems.append(f"  missing: {line}")
            for line in got:
                if line not in want:
                    problems.append(f"  unexpected: {line}")

    if problems:
        print("\nsmoke: the components did not answer as expected.", file=sys.stderr)
        for problem in problems:
            print(f"smoke: {problem}", file=sys.stderr)
        print(
            "\nA wrong answer here is the failure this repo exists to prevent: "
            "the component runs, returns, and lies. See TODO.md#representation.",
            file=sys.stderr,
        )
        return 1

    print("smoke: every example assembles and answers correctly")
    return 0


if __name__ == "__main__":
    sys.exit(main())
