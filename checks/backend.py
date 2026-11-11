#!/usr/bin/env python3
"""Run the generated glue against the module CertiRocq actually emits.

Every other check in this repository stops short of that. `checks.smoke` and
`checks.generated` assemble components, but against each example's `stub/`,
which is a hand-written module standing in for the backend's output;
`checks.representation` compiles with the real CertiRocq but only *reads* the
module it gets. So the one step nothing ran was the join: `wasm-merge` putting
a generated adapter together with a compiled Rocq program and the result
answering correctly. This is layer 2 of TODO.md#representation.

What the join can get wrong is not subtle, and is invisible to the stub path,
because the stub is written to the same `repr.rs` the adapter is: an export
the backend renames, a `call_indirect` type index that moved, a global the
adapter reads as mutable that is not. The stub agrees with the adapter about
all of it by construction.

One case per representation per position, because an export uses each
conversion in one direction only:

    int     succ        a boxed primitive int in both positions, plus the
                        range guard at 2**63 and the wrap at 2**63-1
    bool    to-int      a `bool` argument, with a result of another
            is-zero     representation, and the same the other way round
    enum    is-green    an `enum` argument -- the branch no example under
            colour-of   examples/ has -- and an `enum` result, first case,
                        a middle one and the last
    arity   sub         two parameters, which the backend curries: one
                        application per argument (TODO.md#arity)

Each case is a world and an `impl.v` and nothing else: the interface, the
extraction, the adapter and the build rules are generated, and `make` is what
runs, so the generated build rules are driven for real rather than by
`make -n`.

Half of the join is established by the build getting that far at all, which is
why there is no assertion about it here: the adapter imports the backend's
memory, bump pointer, `out_of_mem` flag, table and `main_function`, and if
`wasm-merge` leaves any of them unresolved -- a name the adapter asks for and
the compiled module does not have -- `wasm-tools component embed` refuses the
module outright ("failed to resolve import `rocq::...`"). The invocations
below are about the other half, where everything resolved and the question is
whether it meant the same thing.

Each pairing is deliberate. `is-green` returns a `bool` rather than cycling
red -> green -> blue, because a cycle survives both conversions being shifted
by the same wrong amount -- a rotation commutes with a rotation -- while
asking which case it is does not. `sub` is not commutative, so applying the
two arguments in the wrong order is an answer this check can see.

The out-of-memory channel runs here too, on the real backend rather than on a
stub with a poisoned bump pointer: the same component, invoked with the host
granting exactly the pages the module declares as its minimum, so the
allocator's `memory.grow` fails, the backend sets `out_of_mem` and the adapter
has to answer `err(out-of-memory)` instead of trapping or answering nonsense.

Needs Rocq, CertiRocq, wasm-tools, binaryen, wasmtime, and the generator:

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


def min_pages(work: Path) -> int:
    """The pages the merged module declares as its minimum."""
    wat = run(
        ["wasm-tools", "parse", str(work / "merged.wasm"), "-t"],
        work,
        "disassembling",
    ).stdout
    found = re.search(
        r"^\s*\(memory(?:\s+\(;\d+;\))?\s+(\d+)(?:\s+\d+)?\)", wat, re.M
    )
    if found is None:
        raise SystemExit(
            f"backend: no memory declaration in {work / 'merged.wasm'}, so the "
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
        cap = min_pages(work) * PAGE
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
