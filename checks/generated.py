#!/usr/bin/env python3
"""Hold the generator to the committed examples, then run what it wrote.

The generated files under `examples/` are committed, so a change to the
emitter shows up as a diff in the pull request that caused it rather than as
output nobody sees. Keeping them committed only means something if they cannot
go stale, which is this check's first half. It generates into a scratch
directory and compares, in two directions that fail differently:

  * **Same bytes.** The generated `interface.v`, `extraction.v`, `adapter.wat`
    and vendored `wit/deps` must equal the committed ones exactly -- comments
    and line breaks included, because those are emitted too and drift in them
    is still drift. Catches an offset, a constant or a discriminant that moved
    -- the mistakes that produce a component which runs and lies -- and
    catches a stale checkout.

    `--update` rewrites the committed copies instead of comparing. That is the
    only supported way to change them: edit the emitter, run it, commit the
    diff.
  * **Same answers.** The generated adapter is swapped into the example and
    assembled against its `stub/`, and the component has to print exactly what
    `checks/smoke.py` says it prints. Catches the code being self-consistently
    wrong, which no byte comparison against the emitter's own output can.
    This is the half with a reference the emitter did not produce: those
    expected lines were measured before the generator existed.

Also runs `make -n` over the generated build rules, which proves they parse and
that every prerequisite resolves without needing Rocq.

Some examples are written *ahead* of the gate, for constructs `generate` still
turns away -- they are the target the support work is written against. Those
have no generated counterpart, so what is checked is the refusal: non-zero
exit, the right rejection code, and nothing written.

Needs the generator on the command line, since the one in the Nix store is the
one the rest of `nix flake check` tested:

    python3 checks/generated.py --generator target/debug/componentize-formal
"""

from __future__ import annotations

import argparse
import difflib
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from smoke import EXPECTED, run_target  # noqa: E402

# The generated files committed into each example, compared byte for byte.
#
# `wit/deps` is in here because the generator vendors the `certirocq:abi`
# package itself -- `generate()` below copies only `wit/world.wit` into the
# scratch directory, so if the generator ever stopped writing the dependency
# this comparison would fail rather than quietly pass on the copy.
#
# The Makefile is not, and is the one file in an example still written by
# hand. The generator emits one, but the committed one carries the `run-stub`
# targets `checks/smoke.py` drives, and emitting test scaffolding is not this
# tool's job. The generated rules are checked by `make -n` instead.
GENERATED = [
    "interface.v",
    "extraction.v",
    "adapter.wat",
    "wit/deps/certirocq-abi/errors.wit",
]

# Examples written ahead of the gate: hand-written throughout, measured, and
# running, for constructs `generate` still turns away. They have no generated
# counterpart to compare against -- what is checked instead is that the
# generator *refuses* them, with the code below. An example the gate silently accepted and emitted
# something for would be worse than one it rejects.
AHEAD_OF_THE_GATE = {
    "variant": "E-STRUCTURED",
}


def generate(generator: Path, example: Path, into: Path) -> None:
    """Generate for the example's world, into an otherwise empty directory.

    Only the *inputs* are copied -- the world and the config. Everything else
    an example holds is either generated here or irrelevant to generating, and
    copying a generated file in first would let the comparison that follows
    pass on the copy.
    """
    (into / "wit").mkdir()
    shutil.copy(example / "wit/world.wit", into / "wit/world.wit")
    shutil.copy(example / "component.toml", into / "component.toml")
    result = subprocess.run(
        [str(generator), "generate", "--config", str(into / "component.toml")],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(
            f"generated: `generate` failed for {example.name} with status "
            f"{result.returncode}:\n{result.stdout}{result.stderr}"
        )


def dry_run(work: Path) -> None:
    """`make -n`: the generated rules parse and every prerequisite resolves."""
    result = subprocess.run(
        ["make", "-n", "all"],
        cwd=work,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(
            "generated: the generated build rules do not resolve "
            f"(`make -n all` exited {result.returncode}):\n"
            f"{result.stdout}{result.stderr}"
        )


def answers(work: Path, target: str) -> tuple[list[str], str | None]:
    """The answers, or why there are none.

    A failed `make` is fatal in `smoke.py`, where it means the toolchain is
    broken. Here it is a finding -- a wrong adapter often does not assemble at
    all -- so it has to be reported alongside the token comparison that
    explains it, not raised over the top of it.
    """
    try:
        return run_target(work, target), None
    except SystemExit as failure:
        return [], str(failure)


def check_example(generator: Path, example: Path) -> list[str]:
    problems: list[str] = []
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "gen"
        out.mkdir()
        generate(generator, example, out)

        for name in GENERATED:
            want = example / name
            got = out / name
            if not got.exists():
                problems.append(f"{example.name}: nothing generated at {name}")
                continue
            if want.read_bytes() != got.read_bytes():
                problems.append(
                    f"{example.name}: the committed {name} is not what the "
                    f"generator writes today. If the emitter changed on "
                    f"purpose, run `checks/generated.py --update` and commit "
                    f"the diff."
                )
                problems.extend(f"  {line}" for line in unified(want, got))
        if not problems:
            print(f"generated: {example.name} matches the committed output", flush=True)

        # `make -n` wants the files the rules mention, which is everything
        # generated plus the implementation the user writes.
        shutil.copy(example / "impl.v", out / "impl.v")
        dry_run(out)
        print(f"generated: {example.name} build rules resolve", flush=True)

        # Now run what was generated, against the example's stub.
        work = Path(tmp) / example.name
        shutil.copytree(example, work)
        for path in [work, *work.rglob("*")]:
            path.chmod(path.stat().st_mode | stat.S_IWUSR)
        shutil.copy(out / "adapter.wat", work / "adapter.wat")
        for target, want in EXPECTED[example.name].items():
            got, failed = answers(work, target)
            if failed is not None:
                problems.append(f"{example.name} {target}: {failed}")
            elif got != want:
                problems.append(f"{example.name} {target}: the generated adapter answers")
                problems.extend(f"  wanted: {line}" for line in want if line not in got)
                problems.extend(f"  got:    {line}" for line in got if line not in want)
            else:
                print(
                    f"generated: {example.name} {target} answers correctly "
                    f"({len(want)} answer(s))",
                    flush=True,
                )
    return problems


def unified(want: Path, got: Path) -> list[str]:
    """The first few lines the committed and generated files part ways on."""
    lines = difflib.unified_diff(
        want.read_text().splitlines(),
        got.read_text().splitlines(),
        fromfile="committed",
        tofile="generated",
        lineterm="",
        n=1,
    )
    return [line for line in lines if line.strip()][:12]


def update(generator: Path, example: Path) -> None:
    """Rewrite the example's committed copies with what the generator emits."""
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "gen"
        out.mkdir()
        generate(generator, example, out)
        for name in GENERATED:
            source = out / name
            if not source.exists():
                raise SystemExit(
                    f"generated: nothing generated at {name} for "
                    f"{example.name}, so there is nothing to update it with."
                )
            target = example / name
            target.parent.mkdir(parents=True, exist_ok=True)
            changed = (
                not target.exists() or target.read_bytes() != source.read_bytes()
            )
            shutil.copy(source, target)
            print(
                f"generated: {'wrote' if changed else 'unchanged'} "
                f"{example.name}/{name}",
                flush=True,
            )


def check_refused(generator: Path, example: Path, code: str) -> list[str]:
    """The generator must turn this example's world away, and say which code.

    These examples exist precisely because the construct is not supported, so
    the thing to verify is the refusal: a non-zero exit, the rejection code in
    the report, and nothing written. Checking the code rather than only the
    exit status is what stops this passing for an unrelated reason -- a typo in
    the world file would also exit non-zero.
    """
    problems = []
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "gen"
        out.mkdir()
        shutil.copytree(example / "wit", out / "wit")
        shutil.copy(example / "component.toml", out / "component.toml")
        result = subprocess.run(
            [str(generator), "generate", "--config", str(out / "component.toml")],
            text=True,
            capture_output=True,
            check=False,
        )
        report = result.stdout + result.stderr
        if result.returncode == 0:
            problems.append(
                f"{example.name}: `generate` succeeded, but this example is "
                f"here because the gate rejects it with {code}. Either the "
                "construct is supported now -- in which case move the example "
                "out of AHEAD_OF_THE_GATE and let the token comparison hold "
                f"the emitter to it -- or the gate has a hole.\n{report}"
            )
        elif code not in report:
            problems.append(
                f"{example.name}: `generate` failed without reporting {code}, "
                f"so it was turned away for some other reason:\n{report}"
            )
        untouched = {"wit", "component.toml"}
        wrote = [p.name for p in out.iterdir() if p.name not in untouched]
        if wrote:
            problems.append(
                f"{example.name}: `generate` was refused but still wrote "
                f"{sorted(wrote)}"
            )
    if not problems:
        print(
            f"generated: {example.name} is refused with {code}, and nothing "
            "written",
            flush=True,
        )
    return problems


def check_fixture(generator: Path, fixture: Path) -> list[str]:
    """Generate for one supported-subset fixture and parse what came out.

    The examples cover two of the shapes the gate accepts. These fixtures are
    the rest of them -- every world `bindgen/tests/wit/good` holds -- and while
    there is no stub to run them against, `wasm-tools parse` still decides
    whether the emitted adapter is a module at all. An adapter that does not
    parse is the one failure mode of a text emitter that no amount of careful
    arithmetic rules out.
    """
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        result = subprocess.run(
            [
                str(generator),
                "generate",
                "--wit",
                str(fixture),
                "--out-dir",
                str(out),
            ],
            text=True,
            capture_output=True,
            check=False,
        )
        if result.returncode != 0:
            return [
                f"{fixture.name}: `generate` failed with status "
                f"{result.returncode}:\n{result.stdout}{result.stderr}"
            ]
        parsed = subprocess.run(
            ["wasm-tools", "parse", str(out / "adapter.wat"), "-o", str(out / "a.wasm")],
            text=True,
            capture_output=True,
            check=False,
        )
        if parsed.returncode != 0:
            return [
                f"{fixture.name}: the generated adapter is not a valid module:\n"
                f"{parsed.stdout}{parsed.stderr}"
            ]
    print(f"generated: {fixture.name} emits an adapter that parses", flush=True)
    return []


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--generator",
        type=Path,
        default=root / "bindgen/target/debug/componentize-formal",
        help="the generator binary to test (default: %(default)s)",
    )
    parser.add_argument("--examples", type=Path, default=root / "examples")
    parser.add_argument(
        "--fixtures",
        type=Path,
        default=root / "bindgen/tests/wit/good",
        help="worlds the gate accepts, one per construct (default: %(default)s)",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="rewrite the committed generated files instead of checking them",
    )
    args = parser.parse_args()

    if not args.generator.exists():
        raise SystemExit(
            f"generated: no generator at {args.generator}. Build it with `cargo "
            "build` in bindgen/, or pass --generator."
        )

    unknown = set(AHEAD_OF_THE_GATE) - set(EXPECTED)
    if unknown:
        raise SystemExit(
            f"generated: AHEAD_OF_THE_GATE names {sorted(unknown)}, which "
            "checks/smoke.py does not know. An example nothing runs is an "
            "example that rots."
        )

    if args.update:
        for name in sorted(EXPECTED):
            if name in AHEAD_OF_THE_GATE:
                print(
                    f"generated: {name} is refused by the gate, so it has no "
                    "generated files to update",
                    flush=True,
                )
                continue
            update(args.generator, args.examples / name)
        return 0

    problems: list[str] = []
    for name in sorted(EXPECTED):
        if name in AHEAD_OF_THE_GATE:
            problems += check_refused(
                args.generator, args.examples / name, AHEAD_OF_THE_GATE[name]
            )
        else:
            problems += check_example(args.generator, args.examples / name)

    fixtures = sorted(args.fixtures.glob("*.wit"))
    if not fixtures:
        raise SystemExit(f"generated: no fixtures in {args.fixtures}")
    for fixture in fixtures:
        problems += check_fixture(args.generator, fixture)

    if problems:
        print(
            "\ngenerated: the generator does not agree with the examples.",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"generated: {problem}", file=sys.stderr)
        print(
            "\nA byte difference is either a deliberate emitter change -- run "
            "`checks/generated.py --update` and commit the diff -- or drift "
            "nobody meant. A wrong *answer* is neither: the expected lines in "
            "checks/smoke.py were measured before the generator existed, so "
            "they are the reference and this is the emitter's mistake to fix. "
            "See TODO.md#representation.",
            file=sys.stderr,
        )
        return 1

    print(
        "generated: every example is either emitted exactly or refused with a "
        "reason"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
