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

# `CertiRocq Compile Wasm --output "conformance"` in the .v decides this.
MODULE_STEM = "conformance"

# One module per constructor, from checks/rocq/constructors.v. Each is a
# function ignoring its argument and returning one constructor, so the
# immediate the backend chose is the only constant in its body.
#
# `bool` is the pair worth pinning: the pipeline swaps its constructors so
# that false/true line up with OCaml's 0/1, even though Rocq declares `true`
# first. `colour` is the control -- three constructors that must come out in
# declaration order, which is what makes `bool` the exception rather than the
# rule. Both are upstream behaviour that can change; an adapter built on the
# wrong one returns the other value, cheerfully.
# A definition that *matches* on an option rather than building one, so the
# test the backend uses to tell a pointer from an immediate can be read back
# out of it. The adapter's lift makes the same test.
MATCH_SOURCE = "ctor-match-option"

# Where each measured immediate lives in bindgen/src/measured.rs, which
# repr.rs holds itself to with a `const` assertion -- so a representation
# change that reaches this file stops the generator compiling.
MEASURED_TARGETS = {
    "ctor-false": ("BOOL_FALSE", None),
    "ctor-true": ("BOOL_TRUE", None),
    "ctor-red": ("NULLARY_ORDINALS", 0),
    "ctor-green": ("NULLARY_ORDINALS", 1),
    "ctor-blue": ("NULLARY_ORDINALS", 2),
    "ctor-mixed-first": ("MIXED_NULLARY_IMMEDIATES", 0),
    "ctor-mixed-last": ("MIXED_NULLARY_IMMEDIATES", 1),
    "ctor-mixed-boxed-a": ("MIXED_BOXED_ORDINALS", 0),
    "ctor-mixed-boxed-b": ("MIXED_BOXED_ORDINALS", 1),
    "ctor-none": ("OPTION_NONE_IMMEDIATE", None),
    "ctor-some": ("OPTION_SOME_ORDINAL", None),
    "ctor-inl": ("SUM_BOXED_ORDINALS", 0),
    "ctor-inr": ("SUM_BOXED_ORDINALS", 1),
    MATCH_SOURCE: ("IMMEDIATE_TAG_BIT", None),
}

CONSTRUCTORS = [
    # (--output stem, repr.rs constant, or the ordinal to expect)
    ("ctor-false", "BOOL_FALSE", None),
    ("ctor-true", "BOOL_TRUE", None),
    ("ctor-red", None, 0),
    ("ctor-green", None, 1),
    ("ctor-blue", None, 2),
    # `mixed`'s two nullary constructors. `m_last` is declaration index 3 and
    # unboxed ordinal 1, which is the whole point of the type: the ordinal is
    # not the declaration index once a constructor with arguments comes first.
    ("ctor-mixed-first", None, 0),
    ("ctor-mixed-last", None, 1),
    # `None`, the one constructor of Stdlib's `option` that allocates nothing.
    # It is the only nullary constructor of its type, so per-class numbering
    # gives it ordinal 0 whichever order `option` declares its two in --- see
    # repr.rs `sums`.
    ("ctor-none", None, 0),
]

# `mixed`'s two constructors *with* arguments, which carry an ordinal rather
# than an immediate. Read by diffing the two modules against each other -- see
# `boxed_ordinals` -- so they come as a pair, with the ordinal each must have.
# Read pairwise, so each pair is two constructors that must differ in exactly
# one constant. `inl`/`inr` are the pair that matters most: both are boxed, so
# their ordinals are declaration order, and a swap upstream would exchange the
# ok and err halves of every WIT `result`. `Some` is read against `inr`
# because it has no boxed sibling of its own to be diffed against -- the two
# modules are the same function but for the constructor, which is all the
# diff needs.
BOXED_PAIRS = [
    (("ctor-mixed-boxed-a", 0), ("ctor-mixed-boxed-b", 1)),
    (("ctor-inl", 0), ("ctor-inr", 1)),
    (("ctor-some", 0), ("ctor-inr", 1)),
]

CONST = re.compile(r'^\s*pub const (?P<name>[A-Z_]+): &str = "(?P<value>[^"]*)";', re.M)
# `pub const BOOL_TRUE: u32 = nullary_immediate(1);`
ORDINAL_CONST = re.compile(
    r"^\s*pub const (?P<name>[A-Z_]+): u32 = nullary_immediate\((?P<ordinal>\d+)\);", re.M
)
# The formula is evaluated below rather than restated, so it is pinned here:
# a change to it must break this check rather than be silently mirrored.
FORMULA = re.compile(
    r"pub const fn nullary_immediate\(ordinal: u32\) -> u32 \{\s*2 \* ordinal \+ 1\s*\}"
)
MEASURED_SCALAR = re.compile(
    r"^pub const (?P<name>[A-Z_]+): u32 = (?P<value>\d+);", re.M
)
MEASURED_ARRAY = re.compile(
    r"^pub const (?P<name>[A-Z_]+): \[u32; \d+\] = \[(?P<values>[^\]]*)\];", re.M
)
FUNC_START = re.compile(r"^  \(func \(;(?P<index>\d+);\)", re.M)
EXPORT_FUNC = re.compile(r'\(export "(?P<name>[^"]*)" \(func (?P<index>\d+)\)\)')
I32_CONST = re.compile(r"^\s*i32\.const (?P<value>-?\d+)\s*$", re.M)
# The boxed/unboxed test a compiled `match` opens with: `x & mask; eqz`, the
# bit being clear selecting the boxed branch (`LambdaANF_to_Wasm.v:349-356`).
# The mask is what repr.rs IMMEDIATE_TAG_BIT claims.
TAG_TEST = re.compile(
    r"i32\.const (?P<mask>\d+)\n\s*i32\.and\n\s*i32\.eqz\n\s*if"
)
# The store of the code pointer into the closure `main_function` allocates:
# `global.get 1` is the record's address, `i32.const 4 / i32.add` its second
# word, and the constant stored there is the function the closure calls. The
# backend's elem section maps function indices to table indices one to one, so
# that constant is both.
CLOSURE_CODE_PTR = re.compile(
    r"global\.get 1\n\s*i32\.const 4\n\s*i32\.add\n"
    r"\s*i32\.const (?P<index>\d+)\n\s*i32\.store"
)
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


def compile_wasm(rocq_source: Path, workdir: Path, stems: list[str] | None = None) -> dict[str, Path]:
    """`rocq c` the file and return the .wasm files the plugin wrote beside it.

    Keyed by the `--output` name in the .v, which is what ties a module back to
    the definition it came from.
    """
    stems = stems or [MODULE_STEM]
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

    modules = {}
    for stem in stems:
        wasm = workdir / f"{stem}.wasm"
        if not wasm.is_file():
            produced = sorted(p.name for p in workdir.iterdir())
            raise Mismatch(
                f"`rocq c` produced no {wasm.name}; the --output names in "
                f"{rocq_source.name} and the stems here must agree. "
                f"The build directory holds: {', '.join(produced)}"
            )
        modules[stem] = wasm
    return modules


def disassemble(wasm: Path) -> str:
    subprocess.run(["wasm-tools", "validate", str(wasm)], check=True)
    return subprocess.run(
        ["wasm-tools", "parse", str(wasm), "-t"],
        check=True,
        text=True,
        capture_output=True,
    ).stdout


def func_bounds(wat: str, stem: str) -> dict[int, tuple[int, int]]:
    """Where each function body starts and ends in the disassembly."""
    starts = [(int(m.group("index")), m.start()) for m in FUNC_START.finditer(wat)]
    if not starts:
        raise Mismatch(
            f"{stem}: found no function bodies, so the pattern this check "
            "reads them with no longer matches wasm-tools' output"
        )
    return {
        index: (start, starts[i + 1][1] if i + 1 < len(starts) else len(wat))
        for i, (index, start) in enumerate(starts)
    }


def returned_immediate(names: dict[str, str], wat: str, stem: str) -> int:
    """The constant a one-constructor function leaves in `result`.

    `main_function` does not compute the definition; it allocates the closure
    for it and leaves that in `result`. The body sits in a separate function,
    reached through the closure's code pointer. The backend exports that
    function, but it exports every other function the pipeline generated too
    -- `reccall_wrapper`, `f_case_known` and friends, under mangled names --
    so the export list does not say which one belongs to the definition. The
    code pointer does, and following it is exactly what the adapter does at
    runtime.

    The wrapper of a function that ignores its argument and returns a nullary
    constructor contains that constructor's immediate and nothing else, so
    there is no need to run anything.
    """
    exported = {m.group("name"): int(m.group("index")) for m in EXPORT_FUNC.finditer(wat)}
    main = exported.get(names["MAIN_FUNCTION"])
    if main is None:
        raise Mismatch(
            f'{stem}: the module exports no function called '
            f'"{names["MAIN_FUNCTION"]}", so the closure this check reads the '
            f"definition's body from cannot be found. It exports: "
            f"{', '.join(sorted(exported)) or '(nothing)'}"
        )

    bounds = func_bounds(wat, stem)
    if main not in bounds:
        raise Mismatch(
            f'{stem}: "{names["MAIN_FUNCTION"]}" is exported as function '
            f"{main}, which has no body in the disassembly"
        )

    start, end = bounds[main]
    pointers = [int(m.group("index")) for m in CLOSURE_CODE_PTR.finditer(wat[start:end])]
    if len(pointers) != 1:
        raise Mismatch(
            f'{stem}: "{names["MAIN_FUNCTION"]}" stores {len(pointers)} code '
            f"pointers ({pointers}), not one. It used to allocate exactly one "
            "closure -- the definition's -- and this check reads the "
            "definition's body through it; the codegen changed and this check "
            "has to be rewritten to follow it."
        )

    wrapper = pointers[0]
    if wrapper not in bounds:
        raise Mismatch(
            f"{stem}: the closure in \"{names['RESULT']}\" points at function "
            f"{wrapper}, which has no body in the disassembly"
        )

    start, end = bounds[wrapper]
    constants = [int(m.group("value")) for m in I32_CONST.finditer(wat[start:end])]
    if len(constants) != 1:
        raise Mismatch(
            f"{stem}: the wrapper (function {wrapper}) holds "
            f"{len(constants)} i32 constants ({constants}), not one. A "
            "function returning a nullary constructor used to compile to "
            "exactly that constant; the codegen changed and this check has to "
            "be rewritten to follow it."
        )
    return constants[0]


def boxed_ordinals(wats: dict[str, str], stems: list[str]) -> dict[str, int]:
    """The ordinals of two constructors that take arguments.

    A function returning `C x` does not compile to a single constant the way a
    nullary one does: the body allocates the block, so the ordinal sits among
    the allocator's own constants with nothing to mark it. But two functions
    differing in nothing except which constructor they build must compile to
    bodies differing in exactly one constant, and that constant is the
    ordinal.

    So this reads them against each other rather than against a pattern. If
    the bodies differ anywhere else, the codegen has changed and the ordinal
    can no longer be read this way -- which is a `Mismatch`, not a finding:
    nothing can be concluded, and guessing would be worse than stopping.
    """
    a, b = stems
    consts = {
        stem: [int(m.group("value")) for m in I32_CONST.finditer(wats[stem])]
        for stem in stems
    }
    if len(consts[a]) != len(consts[b]):
        raise Mismatch(
            f"{a} holds {len(consts[a])} i32 constants and {b} holds "
            f"{len(consts[b])}. They are the same function but for the "
            "constructor, so the bodies have to have the same shape; the "
            "codegen changed and this check has to be rewritten to follow it."
        )
    differ = [i for i, (x, y) in enumerate(zip(consts[a], consts[b])) if x != y]
    if len(differ) != 1:
        raise Mismatch(
            f"{a} and {b} differ in {len(differ)} i32 constants "
            f"(positions {differ}), not one. The ordinal is read as the one "
            "constant that changes when only the constructor does; that no "
            "longer identifies it."
        )
    return {stem: consts[stem][differ[0]] for stem in stems}


def tag_mask(wat: str, stem: str) -> int:
    """The mask a compiled `match` tells a pointer from an immediate with.

    Read off the `match` itself rather than off a constructor: the question
    the adapter's lift has to answer about an `option` is the one the
    backend's own case analysis answers, and the two must answer it the same
    way or they disagree about which constructor a value is.

    Every occurrence has to use the same mask. The definition compiled here
    has one `match` in it, but the pipeline is free to emit more code around
    it, and a module where two tests disagree is one nothing can be concluded
    from.
    """
    masks = {int(m.group("mask")) for m in TAG_TEST.finditer(wat)}
    if not masks:
        raise Mismatch(
            f"{stem}: found no `and`/`eqz` pair opening a case analysis, so "
            "the pattern this check reads the boxed/unboxed test with no "
            "longer matches what the backend emits for a `match`."
        )
    if len(masks) != 1:
        raise Mismatch(
            f"{stem}: the module tests the tag bit against {sorted(masks)} in "
            "different places, so there is no single mask to read."
        )
    return masks.pop()


def measured_values(path: Path) -> dict[str, int]:
    """What bindgen/src/measured.rs records, keyed by --output stem."""
    text = path.read_text()
    scalars = {m.group("name"): int(m.group("value")) for m in MEASURED_SCALAR.finditer(text)}
    arrays = {
        m.group("name"): [int(v) for v in m.group("values").split(",") if v.strip()]
        for m in MEASURED_ARRAY.finditer(text)
    }
    values = {}
    for stem, (name, index) in MEASURED_TARGETS.items():
        try:
            values[stem] = scalars[name] if index is None else arrays[name][index]
        except (KeyError, IndexError):
            raise Mismatch(
                f"{path} no longer records {name}"
                f"{'' if index is None else f'[{index}]'}, which this check "
                "has to compare against. It is generated -- rerun with "
                "--update."
            ) from None
    return values


def write_measured(path: Path, found: dict[str, int]) -> None:
    """Rewrite the generated constants in place, leaving the prose alone."""

    def scalar(match: re.Match[str]) -> str:
        name = match.group("name")
        stem = next(
            (s for s, (n, i) in MEASURED_TARGETS.items() if n == name and i is None), None
        )
        return match.group(0) if stem is None else f"pub const {name}: u32 = {found[stem]};"

    def array(match: re.Match[str]) -> str:
        name = match.group("name")
        items = sorted(
            (i, found[s])
            for s, (n, i) in MEASURED_TARGETS.items()
            if n == name and i is not None
        )
        if not items:
            return match.group(0)
        values = ", ".join(str(v) for _, v in items)
        return f"pub const {name}: [u32; {len(items)}] = [{values}];"

    text = MEASURED_ARRAY.sub(array, MEASURED_SCALAR.sub(scalar, path.read_text()))
    path.write_text(text)


def check_constructors(repr_text: str, names: dict[str, str], wats: dict[str, str]) -> list[str]:
    """Hold repr.rs's constructor encoding to what the backend emits."""
    problems = []
    if not FORMULA.search(repr_text):
        raise Mismatch(
            "repr.rs's `nullary_immediate` is no longer `2 * ordinal + 1`. "
            "This check evaluates that formula to work out what to expect, so "
            "it has to be updated with it."
        )
    ordinals = {
        m.group("name"): int(m.group("ordinal")) for m in ORDINAL_CONST.finditer(repr_text)
    }
    declared_bit = re.search(r"pub const IMMEDIATE_TAG_BIT: u32 = (\d+);", repr_text)
    if declared_bit is None:
        raise Mismatch(
            "repr.rs no longer defines IMMEDIATE_TAG_BIT; this check cannot "
            "verify a constant it cannot read."
        )
    tag_bit = int(declared_bit.group(1))

    found = {}
    for pair in BOXED_PAIRS:
        found.update(boxed_ordinals(wats, [stem for stem, _ in pair]))
    for stem, ordinal in [c for pair in BOXED_PAIRS for c in pair]:
        if found[stem] != ordinal:
            problems.append(
                f"{stem}: the backend uses ordinal {found[stem]}, but "
                f"repr::ordinal::assign gives it {ordinal}. A constructor "
                "with arguments is found by its ordinal in the block's first "
                "cell, so the wrong one names a different constructor."
            )

    found[MATCH_SOURCE] = tag_mask(wats[MATCH_SOURCE], MATCH_SOURCE)
    if found[MATCH_SOURCE] != tag_bit:
        problems.append(
            f"{MATCH_SOURCE}: the backend's own `match` tests the tag bit "
            f"against {found[MATCH_SOURCE]}, but repr.rs IMMEDIATE_TAG_BIT is "
            f"{tag_bit}. A lift that masks with the wrong bit calls a pointer "
            "an immediate, or the other way about, and reads the value at a "
            "constructor it is not."
        )
    for stem, const, ordinal in CONSTRUCTORS:
        found[stem] = returned_immediate(names, wats[stem], stem)
        if const is not None:
            if const not in ordinals:
                raise Mismatch(
                    f"repr.rs no longer defines {const} as "
                    "`nullary_immediate(<ordinal>)`; this check cannot verify "
                    "a constant it cannot read."
                )
            ordinal = ordinals[const]
        want = 2 * ordinal + 1
        got = found[stem]
        if got != want:
            source = f"repr.rs {const}" if const else f"ordinal {ordinal}"
            problems.append(
                f"{stem}: the backend returns {got}, but {source} says "
                f"{want}. An adapter built on the wrong immediate reads the "
                "wrong constructor and answers with a different one."
            )
    return problems, found


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
                    "predating that export cannot be used: move the "
                    "`certirocq` flake input forward -- see "
                    "TODO.md#table-export"
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
        "--constructor-source",
        type=Path,
        default=root / "checks/rocq/constructors.v",
        help="the .v whose constructors are read back (default: %(default)s)",
    )
    parser.add_argument(
        "--measured",
        type=Path,
        default=root / "bindgen/src/measured.rs",
        help="the generated record of what the backend emits "
        "(default: %(default)s)",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="rewrite the measured file from this backend instead of checking "
        "against it. Only after deciding the change is intended: it is what "
        "makes the generator compile again.",
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
        repr_text = args.repr.read_text()
        with tempfile.TemporaryDirectory() as tmp:
            shape = compile_wasm(args.rocq_source, Path(tmp))
            problems = check(names, disassemble(shape[MODULE_STEM]))
        with tempfile.TemporaryDirectory() as tmp:
            stems = [stem for stem, _, _ in CONSTRUCTORS]
            stems += [stem for pair in BOXED_PAIRS for stem, _ in pair]
            stems.append(MATCH_SOURCE)
            stems = list(dict.fromkeys(stems))
            modules = compile_wasm(args.constructor_source, Path(tmp), stems)
            ctor_problems, found = check_constructors(
                repr_text,
                names,
                {stem: disassemble(wasm) for stem, wasm in modules.items()},
            )
            problems += ctor_problems

        if args.update:
            write_measured(args.measured, found)
            print(f"representation: rewrote {args.measured} from this backend")
            for stem, value in found.items():
                print(f"  {stem} = {value}")
            print("Rebuild the generator; repr.rs asserts against these.")
            return 0

        for stem, want in measured_values(args.measured).items():
            if found[stem] != want:
                problems.append(
                    f"{stem}: the backend returns {found[stem]}, but "
                    f"{args.measured.name} records {want}. If the change is "
                    "intended, rerun with --update -- that file is what "
                    "repr.rs asserts against, so the generator will not "
                    "compile until it agrees."
                )
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
    print("type index n taking n i32s, the nullary constructors of `bool`")
    print("(swapped) and a three-case inductive (not swapped), the")
    print("independent boxed and unboxed ordinals of a type mixing both, and")
    print("Stdlib `option` and `sum` -- `None`'s immediate, `Some`, `inl` and")
    print("`inr`'s ordinals, and the mask a compiled `match` tells a pointer")
    print("from an immediate with.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
