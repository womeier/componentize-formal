#!/usr/bin/env python3
"""Run the generated glue against the module CertiRocq actually emits.

Layer 2 of TODO.md#representation, and the one step nothing else ran: the
join. `checks.smoke` and `checks.generated` assemble components, but against
each example's `stub/` -- a hand-written module standing in for the backend --
and `checks.representation` compiles with the real CertiRocq but only *reads*
what it gets. What the join can get wrong is invisible to the stub path,
because the stub is written to the same `repr.rs` the adapter is and agrees
with it by construction: an export the backend renames, a `call_indirect` type
index that moved, a global the adapter reads as mutable that is not.

One case per representation per position, because an export uses each
conversion in one direction only:

    int     succ        a boxed primitive int in both positions, plus the
                        range guard at 2**63 and the wrap at 2**63-1
    bool    to-int      a `bool` argument, with a result of another
            is-zero     representation, and the same the other way round
    enum    is-green    an `enum` argument -- the branch no example under
            colour-of   examples/ has -- and an `enum` result: first case, a
            pick        middle one, the last. `pick` and `bump` add the
            bump        one-case enum, where `2*ord+1` stops describing the
                        value (TODO.md#enum-unit), bare and as a record field
    arity   sub         two parameters, which the backend curries: one
                        application per argument (TODO.md#arity)
    arity6  mix         six, so six applications in a chain -- where a box
                        sized once and shared, so every application reads the
                        same cells, is an answer this case sees six times over
    record  is-empty    a `record` argument through the backend's own compiled
            whole       projections, a `record` result read back through its
            bump        cells, and a nested one, a cell pointing at another
                        block. Two fields everywhere, because a one-field
                        record is not a block at all (TODO.md#record-unboxed)
                        -- which is what `bump` found when its inner one had
                        one
    sum     unwrap-or   an `option` in each position -- one case of each
            halve       CertiRocq kind, so the tag bit is what tells them
            flip        apart -- a `result<u64, bool>` as a *value*, both
            dig         cases boxed so the ordinal picks one, a sum inside a
            shade       sum, and an `enum` inside a sum, where the block's
                        field holds an immediate rather than a pointer

Each case is a world and an `impl.v` and nothing else: the interface, the
extraction, the adapter and the build rules are generated, and `make` runs
them for real rather than through `make -n`.

Half of the join needs no assertion here, because the build does not get this
far without it: the adapter imports the backend's memory, bump pointer,
`out_of_mem` flag, table and `main_function`, and the component instantiates
the adapter with the backend's instance, so `wasm-tools parse` refuses the
assembly if any of them has no matching export. The invocations below are the
other half -- everything resolved, and the question is whether it meant the
same thing.

The pairings are deliberate. `is-green` returns a `bool` rather than cycling
red -> green -> blue, because a cycle survives both conversions being shifted
by the same wrong amount while asking which case it is does not. `sub` is not
commutative, so applying its arguments in the wrong order is an answer this
check can see.

Out-of-memory runs here too, on a real allocator rather than a poisoned bump
pointer: the same component, with the host granting exactly the pages the
module declares as its minimum, so `memory.grow` fails and the adapter has to
answer `err(out-of-memory)` rather than trap or answer nonsense.

Needs Rocq, CertiRocq, wasm-tools, wasmtime, and the generator:

    python3 checks/backend.py --generator target/debug/componentize-formal
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

sys.path.insert(0, str(Path(__file__).resolve().parent))

from roundtrip import repr_variants  # noqa: E402

# One wasm page, which is what `memory.grow` deals in.
PAGE = 65536

# case -> the export's name, what it must answer, and which conversion the
# case is here to pin. `oom` marks the one case that is also run with the heap
# capped; any case would do, since `main_function` allocates before the first
# argument is applied.
EXPECTED: dict[str, dict] = {
    "int": {
        "export": "succ",
        "pins": [("Int63", "param"), ("Int63", "result")],
        "answers": [
            ("0", "ok(1)"),
            ("9223372036854775806", "ok(9223372036854775807)"),
            # `Uint63.int` is modular, so the top of the range wraps rather
            # than failing. The adapter has nothing to report here: the
            # argument had an `int` and so does the answer.
            ("9223372036854775807", "ok(0)"),
            # 2**63 has no `Uint63.int` at all, which is the adapter's range
            # check and the reason every export returns a result
            # (TODO.md#failure-channel).
            ("9223372036854775808", "err(out-of-range)"),
            # The other end of the half that has no counterpart. 2**63 is the
            # boundary and a check written against it passes the case above;
            # this one needs the check to be the comparison it is, so that the
            # whole top half is rejected rather than losing its high bit. All
            # ones is also the value a sign error reads as -1.
            ("18446744073709551615", "err(out-of-range)"),
        ],
        "oom": True,
    },
    "bool-param": {
        "export": "to-int",
        "pins": [("Bool", "param")],
        "answers": [("true", "ok(1)"), ("false", "ok(0)")],
    },
    "bool-result": {
        "export": "is-zero",
        "pins": [("Bool", "result")],
        "answers": [("0", "ok(true)"), ("1", "ok(false)")],
    },
    "enum-param": {
        "export": "is-green",
        "pins": [("Enum", "param")],
        "answers": [
            ("red", "ok(false)"),
            ("green", "ok(true)"),
            ("blue", "ok(false)"),
        ],
    },
    "enum-result": {
        "export": "colour-of",
        "pins": [("Enum", "result")],
        "answers": [("0", "ok(red)"), ("1", "ok(green)"), ("2", "ok(blue)")],
    },
    "sum-param": {
        "export": "unwrap-or",
        "pins": [("Sum", "param")],
        "answers": [
            ("none", "ok(7)"),
            ("some(3)", "ok(3)"),
            # The last `u64` that has a `Uint63.int`. The check is against
            # 2^63, so this one has to pass -- a boundary written one too low
            # rejects a value the Rocq side can hold.
            ("some(9223372036854775807)", "ok(9223372036854775807)"),
            # The payload's range check, which runs under the discriminant
            # that makes the payload live: an `option<u64>` can carry a `u64`
            # with no `Uint63.int` exactly when it is `some`.
            ("some(9223372036854775808)", "err(out-of-range)"),
        ],
    },
    "sum-result": {
        "export": "halve",
        "pins": [("Sum", "result")],
        "answers": [("4", "ok(some(2))"), ("3", "ok(none)"), ("0", "ok(some(0))")],
    },
    # The one-case enum, where `2*ord+1` is not what the backend produces:
    # an informationless constructor is erased, so the value is a block and
    # halving it halves an address. The lift answers with the only
    # discriminant the type has instead (TODO.md#enum-unit).
    "enum-unit": {
        "export": "pick",
        "pins": [("Enum", "param"), ("Enum", "result")],
        "answers": [("only", "ok(only)")],
    },
    # The same enum as a record field, which is where the erasure could have
    # moved the block's layout and does not.
    "enum-unit-field": {
        "export": "bump",
        "pins": [
            ("Enum", "param"),
            ("Enum", "result"),
            ("Record", "param"),
            ("Record", "result"),
        ],
        "answers": [
            ("{tag: only, n: 7}", "ok({tag: only, n: 8})"),
            ("{tag: only, n: 0}", "ok({tag: only, n: 1})"),
            # The field beside it still has its own range check.
            ("{tag: only, n: 9223372036854775808}", "err(out-of-range)"),
        ],
    },
    # And a record every one of whose fields is erased, where the adapter
    # answers without dereferencing either value.
    "enum-unit-both": {
        "export": "pick",
        "pins": [("Record", "param"), ("Record", "result")],
        "answers": [("{a: only, b: only}", "ok({a: only, b: only})")],
    },
    "sum-both": {
        "export": "flip",
        "pins": [("Sum", "param"), ("Sum", "result")],
        "answers": [
            ("ok(5)", "ok(err(false))"),
            ("ok(0)", "ok(err(true))"),
            ("err(true)", "ok(ok(1))"),
            ("err(false)", "ok(ok(0))"),
        ],
    },
    "sum-enum": {
        "export": "shade",
        "pins": [("Sum", "param"), ("Enum", "param"), ("Enum", "result")],
        "answers": [
            ("none", "ok(some(red))"),
            ("some(red)", "ok(some(blue))"),
            ("some(green)", "ok(none)"),
            ("some(blue)", "ok(some(green))"),
        ],
    },
    "sum-nested": {
        "export": "dig",
        "pins": [("Sum", "param"), ("Sum", "result")],
        "answers": [
            ("none", "ok(some(none))"),
            ("some(none)", "ok(none)"),
            ("some(some(4))", "ok(some(some(5)))"),
            # Two discriminants have to hold for the payload to be there, and
            # the range check runs under both.
            ("some(some(9223372036854775808))", "err(out-of-range)"),
        ],
    },
    "record-param": {
        "export": "is-empty",
        "pins": [("Record", "param")],
        "answers": [
            ("{start: 0, byte-length: 5}", "ok(false)"),
            ("{start: 5, byte-length: 0}", "ok(true)"),
            # Every field of a record is part of every value of it, so each
            # one's range check is unguarded -- and one field being in range
            # says nothing about the other.
            ("{start: 9223372036854775808, byte-length: 0}", "err(out-of-range)"),
            ("{start: 0, byte-length: 9223372036854775808}", "err(out-of-range)"),
        ],
    },
    "record-result": {
        "export": "whole",
        "pins": [("Record", "result")],
        "answers": [
            ("3", "ok({start: 3, byte-length: 4})"),
            ("0", "ok({start: 0, byte-length: 1})"),
            # `Uint63.int` is modular, so the second field wraps and the
            # first does not -- the two fields of one answer disagreeing,
            # which a lift reading one cell twice cannot produce.
            ("9223372036854775807", "ok({start: 9223372036854775807, byte-length: 0})"),
            ("9223372036854775808", "err(out-of-range)"),
        ],
    },
    "record-nested": {
        "export": "bump",
        "pins": [("Record", "param"), ("Record", "result")],
        "answers": [
            # A different amount per field, so no two of them can be
            # exchanged without the answer saying so.
            ("{i: {a: 1, z: 1}, b: 2}", "ok({i: {a: 2, z: 3}, b: 12})"),
            ("{i: {a: 0, z: 0}, b: 0}", "ok({i: {a: 1, z: 2}, b: 10})"),
            # Each field two levels down has its own check, reached through
            # both records and guarded by neither.
            ("{i: {a: 9223372036854775808, z: 0}, b: 0}", "err(out-of-range)"),
            ("{i: {a: 0, z: 9223372036854775808}, b: 0}", "err(out-of-range)"),
            ("{i: {a: 0, z: 0}, b: 9223372036854775808}", "err(out-of-range)"),
        ],
    },
    "arity": {
        "export": "sub",
        "pins": [("Int63", "param"), ("Int63", "result")],
        "answers": [
            ("10, 3", "ok(7)"),
            # Not 7: the arguments are applied in declaration order, and this
            # is the answer that says so.
            ("3, 10", "ok(9223372036854775801)"),
            ("9223372036854775808, 0", "err(out-of-range)"),
            # Each parameter is its own check: one being in range says
            # nothing about the other.
            ("0, 9223372036854775808", "err(out-of-range)"),
        ],
    },
    # Six, because the plan-level tests stop at asserting on the emitted text
    # and this is where the applications run for real. The risk the plan tests
    # name -- a box sized once and shared, where the first application
    # captures its cells by address and the last one can still be reading
    # them -- is an answer the alternating sum shows, not something
    # `make -n` can catch.
    "arity6": {
        "export": "mix",
        "pins": [("Int63", "param"), ("Int63", "result")],
        "answers": [
            ("12, 10, 8, 6, 4, 2", "ok(6)"),
            # Not 6: `c` and `d` exchanged, which a plain sum would not
            # notice. Every argument reaches its own parameter or the answer
            # says so.
            ("12, 10, 6, 8, 4, 2", "ok(2)"),
            # Each parameter is its own check: one being in range says
            # nothing about the other five. First, a middle one and the last,
            # so the guard is not somewhere it only works.
            ("9223372036854775808, 10, 8, 6, 4, 2", "err(out-of-range)"),
            ("12, 10, 8, 9223372036854775808, 4, 2", "err(out-of-range)"),
            ("12, 10, 8, 6, 4, 9223372036854775808", "err(out-of-range)"),
        ],
    },
}

# Both positions of every representation have to be covered by some case.
# Derived from plan.rs rather than written out, so a fourth representation
# fails here until two cases exist for it -- running the backend is the only
# place the ordinals and offsets are not taken on trust.
DIRECTIONS = ("param", "result")


def generate(generator: Path, case: Path, work: Path) -> None:
    """Everything but `impl.v`, from the case's world."""
    (work / "wit").mkdir(parents=True)
    shutil.copy(case / "world.wit", work / "wit" / "world.wit")
    shutil.copy(case / "impl.v", work / "impl.v")
    run(
        [str(generator), "generate", "--wit", "wit", "--out-dir", "."],
        work,
        f"{case.name}: generating",
    )


def run(
    argv: list[str], cwd: Path, what: str, must_succeed: bool = True
) -> subprocess.CompletedProcess:
    """A command in the work directory, with its output if it fails.

    `must_succeed` is false for the invocations: a trap is one of the answers
    this check is looking for -- a wrong ordinal makes the compiled match
    execute `unreachable` -- and it belongs in the report for that value
    rather than stopping the run.
    """
    # `rocq c` and `wasmtime` both write under HOME, and in the Nix sandbox
    # HOME is the non-existent /homeless-shelter. Point it at the work
    # directory, as checks/representation.py and checks/smoke.py do.
    home = cwd / "home"
    home.mkdir(exist_ok=True)
    (home / ".cache").mkdir(exist_ok=True)
    result = subprocess.run(
        argv,
        cwd=cwd,
        env=dict(os.environ, HOME=str(home), XDG_CACHE_HOME=str(home / ".cache")),
        text=True,
        capture_output=True,
        check=False,
    )
    if must_succeed and result.returncode != 0:
        raise SystemExit(
            f"backend: {what} failed with status {result.returncode}:\n"
            f"{result.stdout}{result.stderr}"
        )
    return result


def component(work: Path) -> Path:
    """The component `make` built, whatever the configured name is."""
    built = sorted(work.glob("*.comp.wasm"))
    if len(built) != 1:
        raise SystemExit(
            f"backend: `make` produced {len(built)} *.comp.wasm files in "
            f"{work}, wanted exactly one: {[p.name for p in built]}"
        )
    return built[0]


def invoke(comp: Path, call: str, work: Path, cap: int | None = None) -> str:
    """Call the export, and return the one line `wasmtime` printed.

    `cap` limits the memory the host will hand out, in bytes, which is how the
    out-of-memory channel is reached without a Rocq program that allocates a
    gigabyte: the pooling allocator is the knob wasmtime offers for it.
    """
    limit = (
        ["-O", "pooling-allocator=y", "-O", f"pooling-max-memory-size={cap}"]
        if cap is not None
        else []
    )
    result = run(
        ["wasmtime", "run", *limit, "--invoke", call, str(comp)],
        work,
        f"`{call}`",
        must_succeed=False,
    )
    if result.returncode != 0:
        # A trap is an answer here, and which trap matters: `unreachable` is
        # what a compiled `match` does when handed an ordinal no constructor
        # has, which is what a wrong immediate looks like from outside.
        trap = re.search(r"wasm trap: (.*)", result.stderr)
        return f"trap({trap.group(1).strip()})" if trap else "did not run"
    answers = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    if len(answers) != 1:
        raise SystemExit(
            f"backend: `{call}` printed {len(answers)} values, wanted one:\n"
            f"{result.stdout}"
        )
    return answers[0]


def min_pages(comp: Path, work: Path) -> int:
    """The pages the backend declares as its memory's minimum.

    Read off the built component: the backend is embedded in it unmodified,
    and its memory is the only memory *declaration* in there -- the adapter
    only imports one, and an import prints as an `(import ...)` line this
    pattern does not match.
    """
    wat = run(
        ["wasm-tools", "print", str(comp)],
        work,
        "disassembling",
    ).stdout
    found = re.search(
        r"^\s*\(memory(?:\s+\(;\d+;\))?\s+(\d+)(?:\s+\d+)?\)", wat, re.M
    )
    if found is None:
        raise SystemExit(
            f"backend: no memory declaration in {comp}, so the "
            "heap cannot be capped at its minimum. The out-of-memory case "
            "needs one."
        )
    return int(found.group(1))


def check_case(generator: Path, case: Path, work: Path) -> list[str]:
    """One world, generated, compiled, merged, assembled, and invoked."""
    want = EXPECTED[case.name]
    export = want["export"]
    generate(generator, case, work)
    run(["make", "-s", "all"], work, f"{case.name}: `make all`")
    comp = component(work)

    problems: list[str] = []
    for args, answer in want["answers"]:
        call = f"{export}({args})"
        got = invoke(comp, call, work)
        if got != answer:
            problems.append(f"{case.name}: {call} = {got}, wanted {answer}")

    if want.get("oom"):
        # Exactly the minimum: the module instantiates, and the first
        # `memory.grow` the allocator asks for fails.
        cap = min_pages(comp, work) * PAGE
        args = want["answers"][0][0]
        call = f"{export}({args})"
        got = invoke(comp, call, work, cap=cap)
        if got != "err(out-of-memory)":
            problems.append(
                f"{case.name}: {call} with the heap capped at {cap} bytes = "
                f"{got}, wanted err(out-of-memory). The backend sets "
                "`out_of_mem` when `memory.grow` fails and the adapter has to "
                "report it rather than return whatever `result` happens to hold"
            )

    # The generated `run` rule, driven once rather than only parsed by the
    # `make -n` in checks.generated: it is the rule a user of this tool types.
    args = want["answers"][0][0]
    out = run(
        ["make", "-s", "run", f"ARGS={args}"], work, f"{case.name}: `make run`"
    ).stdout
    if want["answers"][0][1] not in out:
        problems.append(
            f"{case.name}: `make run ARGS={args}` printed {out.strip()!r}, "
            f"wanted {want['answers'][0][1]}"
        )

    if not problems:
        print(
            f"backend: {case.name} {export} answers "
            f"{len(want['answers'])} case(s)"
            + (" and reports out-of-memory" if want.get("oom") else "")
        )
    return problems


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
        "--cases",
        type=Path,
        default=root / "checks/backend",
        help="one directory per case, each a world.wit and an impl.v "
        "(default: %(default)s)",
    )
    parser.add_argument(
        "--plan",
        type=Path,
        default=root / "bindgen/src/emit/plan.rs",
        help="where the list of representations lives (default: %(default)s)",
    )
    args = parser.parse_args()

    # Every command runs in a work directory of its own, so a relative path
    # on the command line has to be resolved before anything changes
    # directory under it.
    args.generator = args.generator.resolve()
    if not args.generator.exists():
        raise SystemExit(
            f"backend: no generator at {args.generator}. Build it with `cargo "
            "build` in bindgen/, or pass --generator."
        )
    cases = sorted(p for p in args.cases.iterdir() if p.is_dir())
    if not cases:
        raise SystemExit(f"backend: no cases in {args.cases}")

    named = set(EXPECTED)
    on_disk = {case.name for case in cases}
    if named != on_disk:
        raise SystemExit(
            "backend: the cases on disk and the expected answers disagree. "
            f"Only on disk: {sorted(on_disk - named)}. Only expected: "
            f"{sorted(named - on_disk)}. A case with no expectations runs "
            "nothing, and an expectation with no case checks nothing."
        )

    covered = {pin for want in EXPECTED.values() for pin in want["pins"]}
    wanted = {
        (variant, direction)
        for variant in repr_variants(args.plan)
        for direction in DIRECTIONS
    }
    missing = wanted - covered
    if missing:
        raise SystemExit(
            "backend: no case runs "
            + ", ".join(f"{variant} as a {direction}" for variant, direction in sorted(missing))
            + " against the real backend. Add a world and an impl.v under "
            f"{args.cases} -- a representation that has only ever met a stub "
            "is one where repr.rs is taken on trust in the one place it could "
            "be checked."
        )

    problems: list[str] = []
    for case in cases:
        with tempfile.TemporaryDirectory() as tmp:
            problems += check_case(args.generator, case, Path(tmp) / "work")

    if problems:
        print(
            "\nbackend: the generated glue and the compiled module disagree.",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"backend: {problem}", file=sys.stderr)
        print(
            "\nThis is the one check that runs both halves for real, so a "
            "failure here is either the adapter or repr.rs being wrong about "
            "the module CertiRocq emits -- start with checks.representation, "
            "which says which claim about it moved. See TODO.md#representation.",
            file=sys.stderr,
        )
        return 1

    print(
        f"backend: {len(cases)} world(s) compiled with the real CertiRocq, "
        f"merged with their generated adapter, and answering"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
