# `variant`

A `variant` crossing the boundary in both directions — the example where the
two sides disagree about how to number cases:

```wit
variant byte-count {
    unknown,
    exactly(u64),
    at-least(u64),
    too-big,
}

export add-byte: func(c: byte-count) -> result<byte-count, error>;
```

**The gate rejects this world today** (`E-STRUCTURED`,
[`TODO.md#structured-types`](../../TODO.md#structured-types)), so
`componentize-formal generate` cannot produce these files. They are
hand-written in the shape it will have to emit, measured against the real
backend, and they run.

```console
$ nix develop
$ cd examples/variant && make run-stub
add-byte(unknown) = ok(at-least(1))
add-byte(exactly(41)) = ok(exactly(42))
add-byte(at-least(7)) = ok(at-least(8))
add-byte(exactly(9223372036854775807)) = ok(too-big)
add-byte(too-big) = ok(too-big)
add-byte(exactly(9223372036854775808)) = err(out-of-range)
$ make run-stub-oom
add-byte(exactly(41)) with the heap exhausted = err(out-of-memory)
```

Those six cover every direction a case can move: unboxed in and boxed out
(`unknown`), boxed to boxed (`exactly`, `at-least`), boxed in and unboxed out
(the overflow), and unboxed to unboxed (`too-big`).

## The table, and why it is a table

Every number below was read off a module the real CertiRocq compiled, not
worked out from the discriminant:

| WIT discriminant | case | CertiRocq representation |
| --- | --- | --- |
| 0 | `unknown` | immediate `1` — unboxed ordinal 0 |
| 1 | `exactly(u64)` | `[0, &payload]` — boxed ordinal 0 |
| 2 | `at-least(u64)` | `[1, &payload]` — boxed ordinal 1 |
| 3 | `too-big` | immediate `3` — unboxed ordinal 1 |

`match_ordinals_with_tag` numbers boxed and unboxed constructors with
**independent counters**
([`repr.rs`](../../bindgen/src/repr.rs)'s `ordinal::assign`), so the two
classes reuse ordinals 0 and 1 and the low tag bit is the only thing that tells
them apart. There is no arithmetic from the WIT discriminant that produces that
column — which is exactly why `enum` could get away with `2*d+1` and `variant`
cannot. The adapter writes the four cases out one at a time, in both
directions.

It is load-bearing. Lift `exactly` with ordinal 1 instead of 0 and the
component answers `add-byte(exactly(41)) = ok(at-least(42))`: it assembles, it
runs, and it lies.

## What else is new here, over [`record`](../record)

**`too-big` is not a failure.** A `u64` of 2⁶³ or more is `err(out-of-range)`,
because no value of the Rocq type stands for it. A count that overflows *while
being added to* is a case the world declares, and `impl.v` decides it with
`if ((n + 1) =? 0)%uint63`. The two failures are different and the interface
keeps them apart; the error channel carries only what the adapter owns.

**The payload is checked where there is a payload.** A variant flattens to its
discriminant plus the join of its payloads, so `byte-count` arrives as an `i32`
and an `i64` — and for `unknown` and `too-big` that `i64` is not part of the
value. Whatever the canonical ABI puts in the slot is none of the adapter's
business, and checking it anyway would mean relying on what that is.

**Five `unreachable`s, where the other examples have one.** Worth listing,
because a trap is a worse answer than a reported failure and each one needs a
reason there is nothing to report:

1. the reservation does not fit at the bottom of a fresh heap — a failure with
   nowhere to write it, as in [`plus1`](../plus1);
2. a discriminant of 4 or more on the way in — it names no case, so there is
   no Rocq value to build;
3. the lift falls through all four cases — dead, since (2) has already run,
   and there so that a case added to the world and forgotten here cannot land
   in whichever branch happens to be last;
4. and 5. the lowering meets a value that is neither of the immediates nor
   either ordinal. These two are the backend's own: its compiled `match`
   executes `unreachable` in exactly that position. An adapter more forgiving
   than the module it is linked into would turn a representation that had
   drifted into a wrong answer instead of a stop.

`result<byte-count, error>` is 24 bytes, align 8: the outer discriminant at 0,
the payload at 8 — so `byte-count`'s own discriminant lands at 8 and its `u64`
at 16, and `error`, being one byte, also lands at 8.

## Measured, not inferred

`rocq c extraction.v` works on the current pin — only the link step waits on
[`TODO.md#table-export`](../../TODO.md#table-export) — so the module is there
to look at, and [`stub/backend.wat`](stub/backend.wat) mirrors what it does:
the dispatch on the tag bit, the `unreachable` on anything unrecognised, the
mask to 63 bits, and the closure `[0, 1, 0]`. A stub more forgiving than the
thing it stands in for would carry an adapter the real backend rejects.

Both stubs arrive with a **dirty heap**, a `(data ...)` segment filling the
bytes the call reaches with `0xa5`, for the same reason as in
[`record`](../record): over a zeroed page the ordinal cell the adapter writes
would already read as 0, which is `exactly` — the case most of these calls
want anyway.

[`stub/oom.wat`](stub/oom.wat) runs out of heap *during* the call, which
[`plus1`](../plus1)'s cannot. Delete the adapter's second out-of-memory check
and the component answers `ok(exactly(0))`: `result` still holds the closure
from before, and a closure is an even pointer whose first cell is the tag 0,
which the lowering reads as `exactly`.

## The files

| | Who writes it |
| --- | --- |
| [`wit/world.wit`](wit/world.wit) | you — the interface comes first |
| [`wit/deps/certirocq-abi/`](wit/deps/certirocq-abi) | vendored, because resolving the world needs the package on disk beside it |
| [`interface.v`](interface.v) | to be generated — the world as an inductive and a `Module Type` |
| [`impl.v`](impl.v) | **you** — the only file that is yours |
| [`extraction.v`](extraction.v) | to be generated — the `CertiRocq Compile Wasm` call |
| [`adapter.wat`](adapter.wat) | to be generated — the canonical-ABI glue |
| [`component.wat`](component.wat) | to be generated — the component shell: the world's type, around the two core modules |
| [`Makefile`](Makefile) | to be generated — `rocq c`, then splice the backend and the adapter into `component.wat` |
| [`component.toml`](component.toml) | you — what the WIT file cannot say |
| [`stub/`](stub) | neither; it stands in for CertiRocq's output |

## Known gaps

- **One payload per case, and it is a `u64`.** A case carrying a record, or
  several fields, is the rest of
  [`TODO.md#structured-types`](../../TODO.md#structured-types): the Rocq side
  gets more cells, and the canonical-ABI side gets a payload whose size is the
  max over the cases.
- **The lift is a chain of comparisons**, which is what reads best by hand. A
  generator with more than a handful of cases wants `br_table`; the table it
  branches on is the same one.
- **Every call re-runs `main_function`**, as in the other examples: the fix is
  the [heap reset](../../TODO.md#heap-reset).
