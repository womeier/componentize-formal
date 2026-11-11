#!/usr/bin/env python3
"""Run every conversion the generator can emit in both directions, and require
the value back unchanged.

`bindgen/src/emit/convert.rs` holds one pair of conversions per representation
-- a canonical-ABI value into the one CertiRocq uses, and back. An export
adapter only ever uses each direction in one position: a parameter comes in,
a result goes out. The other order is what a function *import* will need
(`TODO.md#imports`), and the gate still refuses imports, so nothing in the
repository runs it.

This check runs it. `componentize-formal roundtrip` emits a small module that
composes each pair both ways, with a manifest of what to call and with what,
and every call here has to answer with what it was given:

    canonical_<type>   v  ->  to_rocq    ->  from_rocq  ->  v
    rocq_<type>        x  ->  from_rocq  ->  to_rocq    ->  x

The identity is the whole expectation, which is why the manifest carries no
expected values -- a round trip has nothing to compare against but its own
input, and a manifest that also carried the answer could agree with a broken
generator about both.

What this check does restate, independently of the generator, is which values
it refuses to run without:

  * **`int`**: 0 and 2**63-1, the ends of the range `Uint63.int` has. A
    conversion that drops or sign-extends the top bit is right in the middle
    of the range and wrong at the end of it. Anything at 2**63 or above is
    *not* run: it has no `Uint63.int`, which is the adapter's range check to
    answer for and not the conversion's (`TODO.md#failure-channel`).
  * **`bool`**: both of 0 and 1.
  * **an `enum`**: every case, because `2*ord+1` is per case and a shift that
    is right for the first can be wrong for the last.
  * **every immediate, against the formula**: the Rocq-side values the
    manifest offers have to be `2*ord+1` of the canonical ones. Computed here
    rather than read from the manifest, so a generator that moved the formula
    and moved its own values with it still fails.

Worlds come from `bindgen/tests/wit/good` -- every shape the gate accepts, one
fixture each -- and the run ends by checking that each variant of
`plan.rs`'s `Repr` was reached, so a representation added without a fixture
fails here instead of going quietly untested. A representation that has no
round trip to run says so in `EXCLUDED`, with what runs it instead; `option`
and `result` are there, because a sum is several core values rather than one.

Needs the generator and `wasmtime`:

    python3 checks/roundtrip.py --generator target/debug/componentize-formal
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

# The boundary values this check will not run a representation without. Keyed
# by what `Repr` variant the pair came from; `None` means the cases are
# decided by the world rather than by the representation, and are checked for
# being a complete run instead.
REQUIRED = {
    "Int63": {"0", "9223372036854775807"},
    "Bool": {"0", "1"},
    "Enum": None,
}

# The representations this harness cannot carry, and why. A round trip needs
# the value to be one core value in each direction, and a sum is not one: its
# canonical form is a discriminant and a payload at byte offsets, so the
# conversion writes into memory instead of returning something to compare.
# Both of its directions run against the real backend instead, in
# `checks/backend.py`'s sum cases -- which is a weaker statement than an
# identity, and is the one being made.
#
# Listed rather than ignored: a variant that is neither required nor excluded
# fails the parse below.
EXCLUDED = {"Sum"}

# Every variant of `Repr` has to be reached by some fixture. The map is from
# the variant name in plan.rs to what a pair from it looks like in the
# manifest; `classify` is the other half of it.
#
# This is a restatement, which is the point: when plan.rs grows a fourth
# representation, the parse below finds a variant this table does not name and
# the check fails, rather than passing because no fixture happened to use it.
VARIANTS = set(REQUIRED)


def classify(pair: dict) -> str:
    """Which `Repr` variant this manifest entry came from.

    By the two things that distinguish them from outside: a boxed `int` is the
    only representation carried in an `i64`, and `bool` is the only immediate
    whose Rocq type is named by the frontend rather than by the world.
    """
    if pair["core"] == "i64":
        return "Int63"
    return "Bool" if pair["rocq_type"] == "bool" else "Enum"


def repr_variants(plan: Path) -> set[str]:
    """The variants of `enum Repr`, read out of plan.rs."""
    text = plan.read_text()
    body = re.search(r"pub enum Repr \{(.*?)\n\}", text, re.S)
    if body is None:
        raise SystemExit(
            f"roundtrip: no `pub enum Repr` in {plan}. This check asserts that "
            "every representation is reached by a fixture, and it cannot do "
            "that without the list of them."
        )
    return set(re.findall(r"^\s{4}([A-Z]\w*)", body.group(1), re.M))


def harness(generator: Path, world: Path, into: Path) -> tuple[Path, dict]:
    """Emit the harness module and manifest for one world."""
    module = into / f"{world.stem}.wat"
    manifest = into / f"{world.stem}.toml"
    result = subprocess.run(
        [
            str(generator),
            "roundtrip",
            "--wit",
            str(world),
            "--out",
            str(module),
            "--manifest",
            str(manifest),
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(
            f"roundtrip: `roundtrip` failed for {world.name} with status "
            f"{result.returncode}:\n{result.stdout}{result.stderr}"
        )
    return module, tomllib.loads(manifest.read_text())


def invoke(module: Path, func: str, value: str) -> str:
    """Call one export of the harness, and return what it answered.

    `--invoke` on a core module with arguments is experimental in `wasmtime`,
    which is accepted here deliberately: the alternative is a harness that
    tests itself internally and can only report *that* something came back
    wrong, where this reports which value and in which direction. The
    toolchain is pinned by revision (`TODO.md#pinning`), so the warning is not
    a moving target.
    """
    result = subprocess.run(
        ["wasmtime", "run", "--invoke", func, str(module), value],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(
            f"roundtrip: {func}({value}) did not run:\n"
            f"{result.stdout}{result.stderr}"
        )
    answers = [line for line in result.stdout.splitlines() if line.strip()]
    if len(answers) != 1:
        raise SystemExit(
            f"roundtrip: {func}({value}) printed {len(answers)} values, wanted "
            f"one:\n{result.stdout}"
        )
    return answers[0].strip()


def immediate(ordinal: int) -> int:
    """`2*ord+1`, the nullary constructor's immediate. repr.rs."""
    return 2 * ordinal + 1


def check_values(pair: dict, variant: str) -> list[str]:
    """The manifest offers the values this check insists on, and no others."""
    problems = []
    canonical = pair["canonical_values"]
    want = REQUIRED[variant]
    if want is not None:
        missing = want - set(canonical)
        if missing:
            problems.append(
                f"{pair['rocq_type']}: the harness does not round-trip "
                f"{sorted(missing)}, which is where a conversion goes wrong if "
                "it goes wrong at all"
            )
    else:
        # An enum's cases come from the world, so what is checked is that all
        # of them are there: 0..n-1 with nothing skipped, and more than one,
        # since a single case cannot catch a shift.
        ordinals = sorted(int(v) for v in canonical)
        if ordinals != list(range(len(ordinals))) or len(ordinals) < 2:
            problems.append(
                f"{pair['rocq_type']}: the canonical cases are {ordinals}, "
                "wanted every ordinal from 0 up, at least two of them"
            )

    if variant == "Int63":
        # 2**63 and above has no `Uint63.int`; converting one rather than
        # refusing it is the bug the adapter's range check exists for, and
        # round-tripping one here would say it is fine.
        out = [v for v in canonical if int(v) >= 2**63]
        if out:
            problems.append(
                f"{pair['rocq_type']}: {out} is out of range for Uint63.int, "
                "so the conversion is not what should be run on it"
            )
        if pair["rocq_values"] != canonical:
            problems.append(
                f"{pair['rocq_type']}: a boxed payload is the same number on "
                "both sides, but the manifest gives different values"
            )
    else:
        # The formula, computed here rather than taken from the manifest.
        want_rocq = [str(immediate(int(v))) for v in canonical]
        if pair["rocq_values"] != want_rocq:
            problems.append(
                f"{pair['rocq_type']}: the Rocq-side values are "
                f"{pair['rocq_values']}, but 2*ord+1 of {canonical} is "
                f"{want_rocq}"
            )
    return problems


def check_world(generator: Path, world: Path, into: Path) -> tuple[list[str], set[str]]:
    problems: list[str] = []
    reached: set[str] = set()
    module, manifest = harness(generator, world, into)
    if not manifest.get("pair"):
        # Nothing to run is an answer only when the manifest says what it
        # passed over and why; an empty manifest with no reason in it is a
        # generator that emitted nothing, which is what this check is for.
        skipped = manifest.get("skipped")
        if not skipped:
            return [f"{world.name}: the harness has no conversions in it"], reached
        for entry in skipped:
            print(
                f"roundtrip: {world.name} carries only `{entry['wit']}`, which has "
                f"no round trip: {entry['reason']}"
            )
        return problems, reached

    for pair in manifest["pair"]:
        variant = classify(pair)
        if variant not in REQUIRED:
            problems.append(f"{world.name}: unclassifiable pair {pair}")
            continue
        reached.add(variant)
        found = check_values(pair, variant)

        # Both directions, every value. A failure names the direction because
        # the two are not symmetric: one goes through memory and one does not.
        for func, values in (
            (pair["canonical"], pair["canonical_values"]),
            (pair["rocq"], pair["rocq_values"]),
        ):
            for value in values:
                got = invoke(module, func, value)
                if got != value:
                    found.append(
                        f"{world.name}: {func}({value}) came back {got}. The "
                        "two conversions are not inverses, so an import "
                        "wrapper built on them would not be either."
                    )
        problems += found
        if not found:
            print(
                f"roundtrip: {world.stem} `{pair['rocq_type']}` is the identity "
                f"both ways on {len(pair['canonical_values'])} value(s)",
                flush=True,
            )
    return problems, reached


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--generator",
        type=Path,
        default=root / "bindgen/target/debug/componentize-formal",
        help="the generator binary to test (default: %(default)s)",
    )
    parser.add_argument(
        "--fixtures",
        type=Path,
        default=root / "bindgen/tests/wit/good",
        help="worlds the gate accepts, one per construct (default: %(default)s)",
    )
    parser.add_argument(
        "--plan",
        type=Path,
        default=root / "bindgen/src/emit/plan.rs",
        help="where the list of representations lives (default: %(default)s)",
    )
    args = parser.parse_args()

    if not args.generator.exists():
        raise SystemExit(
            f"roundtrip: no generator at {args.generator}. Build it with `cargo "
            "build` in bindgen/, or pass --generator."
        )
    fixtures = sorted(args.fixtures.glob("*.wit"))
    if not fixtures:
        raise SystemExit(f"roundtrip: no fixtures in {args.fixtures}")

    unknown = repr_variants(args.plan) - VARIANTS - EXCLUDED
    if unknown:
        raise SystemExit(
            f"roundtrip: plan.rs has representation(s) {sorted(unknown)} that "
            "this check does not know how to recognise or what boundary values "
            "to insist on. Add them to REQUIRED and `classify`, with a fixture "
            "in the good set that uses them -- a conversion nothing round-trips "
            "is one the import work will find out about the hard way. If the "
            "representation cannot round-trip at all, say so in EXCLUDED with "
            "what runs it instead."
        )

    problems: list[str] = []
    reached: set[str] = set()
    with tempfile.TemporaryDirectory() as tmp:
        for fixture in fixtures:
            found, got = check_world(args.generator, fixture, Path(tmp))
            problems += found
            reached |= got

    missed = VARIANTS - reached
    if missed:
        problems.append(
            f"no fixture reaches {sorted(missed)}, so {'its' if len(missed) == 1 else 'their'} "
            "conversion was never run"
        )

    if problems:
        print("\nroundtrip: the conversions are not inverses.", file=sys.stderr)
        for problem in problems:
            print(f"roundtrip: {problem}", file=sys.stderr)
        print(
            "\nBoth directions come out of bindgen/src/emit/convert.rs, and an "
            "export adapter uses them in the order the round trip's first half "
            "uses them -- so a failure here can mean the emitted adapters are "
            "wrong too, not only the direction imports will need. See "
            "TODO.md#imports.",
            file=sys.stderr,
        )
        return 1

    print(
        f"roundtrip: every conversion in {len(fixtures)} world(s) is the "
        f"identity both ways, over {len(reached)} representation(s)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
