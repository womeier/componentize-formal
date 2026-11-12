# `record`

A `record` crossing the boundary in both directions:

```wit
record span {
    start: u64,
    byte-length: u64,
}

export grow: func(s: span) -> result<span, error>;
```

Everything here except `impl.v` was written by hand in the shape
`componentize-formal generate` would have to emit, measured against the real
backend, and run — before the gate accepted a `record` at all. The emitter has
since caught up, so the glue committed here is its output, held to the byte by
`checks/generated.py`; see [`plus1`](../plus1) for which files those are.

```console
$ nix develop
$ cd examples/record && make run-stub
grow({start:10,byte-length:1}) = ok({start: 10, byte-length: 2})
grow({start:0,byte-length:0}) = ok({start: 0, byte-length: 1})
grow({start:7,byte-length:9223372036854775807}) = ok({start: 7, byte-length: 0})
grow({start:9223372036854775808,byte-length:0}) = err(out-of-range)
grow({start:0,byte-length:9223372036854775808}) = err(out-of-range)
$ make run-stub-oom
grow({start:10,byte-length:1}) with the heap exhausted = err(out-of-memory)
```

The last three lines are the ones to read. `byte-length` of 2⁶³-1 grows to 0
because `Uint63` wraps, as in `plus1`. The two `err(out-of-range)` lines differ
only in *which* field is out of range: a record carries no `u64` that is
exempt, so every field is its own check, and one of them firing is not evidence
about the other.

`grow` deliberately touches its two fields differently. An operation that
treated them alike — a swap, say — would hide a swapped pair of field offsets
completely, because lifting and lowering with the same mistake cancels out and
the answer comes back right.

## What is new here, over `plus1`

**The adapter builds a value.** A WIT record of two `u64`s *flattens* into two
core `i64` parameters, so nothing arrives by pointer; the Rocq side wants a
boxed constructor with a pointer per field. So the adapter allocates three
things instead of one: a box per field, and the record block pointing at them.

```
$ret + 0   .. 24   the canonical-ABI return area
$arg + 0   .. 12   the `span` itself: [ordinal, &start, &byte-length]
$arg + 12  .. 20   `start`, boxed
$arg + 20  .. 28   `byte-length`, boxed
```

The field boxes live inside the argument's *own* block, addressed off `$arg`
rather than off `$ret`. It reads as a detail at one parameter and is not one at
two: every parameter's cells have to be its own, or a second record would
write over the first one's fields.

**The ordinal is written, not assumed.** A record has one constructor, so its
ordinal is 0 — and the compiled projections load it and execute `unreachable`
on anything else, so leaving the cell alone is a trap rather than a wrong
answer. Both stubs therefore arrive with a **dirty heap**: a `(data ...)`
segment fills the bytes the call reaches with `0xa5`. Over a zeroed page an
adapter that never wrote the ordinal would pass, because the cell it forgot
would already hold the value it needed.

**Every field is a double load on the way back.** `result` holds a pointer to
the returned record, whose cells are pointers to the boxed fields, so `$ok`
takes the Rocq value and does both loads itself:
`i64.load (i32.load offset=4 (local.get $val))`. Handing `$ok` one core
parameter per field would read more simply and not generalize — a field that is
an `option` has no single core value to hand it.

**The second out-of-memory check is the one that fires.**
[`stub/oom.wat`](stub/oom.wat) has a heap that runs out *during* the call
rather than before it, which [`plus1`](../plus1)'s cannot: there the flag is
already set when `main_function` returns. Delete that check and the component
answers `ok({start: 0, byte-length: 0})` — it runs, returns, and lies, which is
the failure this repo exists to prevent.

`result<span, error>` is 24 bytes, align 8: a one-byte discriminant at offset
0, the payload at offset 8, and `span`'s two fields at 8 and 16 in declaration
order.

## Measured, not inferred

The representation above was read off a module the real CertiRocq compiled, not
worked out from the sources. `rocq c extraction.v` works on the current pin —
only the link step waits on [`TODO.md#table-export`](../../TODO.md#table-export)
— so the module is there to look at, and it says:

- a `Record` is an ordinary boxed constructor: ordinal at offset 0, field *i*
  at `4*(i+1)`, each cell a pointer to an 8-byte boxed `int`;
- each field is read through its own table function, which traps unless the
  pointer is even and the ordinal is 0;
- the body reuses the caller's box for the field it does not change, allocating
  only for the one it does;
- the closure is `[0, 3, 0]` — `main_function` builds it exactly as in `plus1`.

[`stub/backend.wat`](stub/backend.wat) mirrors all four, including the traps.
A stub more forgiving than the thing it stands in for would carry an adapter
that the real backend rejects.

The Rocq half of `make run` therefore works today, and stops where `plus1`'s
does — against a CertiRocq that does not export its function table, which is
every release ([`TODO.md#table-export`](../../TODO.md#table-export)):

```console
$ make run
rocq c interface.v
rocq c impl.v
rocq c extraction.v
wasm-tools parse adapter.wat -o adapter.wasm
error: this CertiRocq does not export its function table as
       "table", which the adapter links the compiled closures
       through. Move the `certirocq` flake input forward -- see
       TODO.md#table-export. `make run-stub` exercises the
       adapter without it.
```

## The files

| | Who writes it |
| --- | --- |
| [`wit/world.wit`](wit/world.wit) | you — the interface comes first |
| [`wit/deps/certirocq-abi/`](wit/deps/certirocq-abi) | vendored, because `wasm-tools component embed` reads the package off disk |
| [`interface.v`](interface.v) | generated — the world as a `Record` and a `Module Type` |
| [`impl.v`](impl.v) | **you** — the only file that is yours |
| [`extraction.v`](extraction.v) | generated — the `CertiRocq Compile Wasm` call |
| [`adapter.wat`](adapter.wat) | generated — the canonical-ABI glue |
| [`Makefile`](Makefile) | generated — `wasm-merge`, `component embed`, `component new` |
| [`component.toml`](component.toml) | you — what the WIT file cannot say |
| [`stub/`](stub) | neither; it stands in for CertiRocq's output |

## Known gaps

- **Nothing here nests**, though the emitter does it: `span`'s fields are both
  `u64`. A record field that is itself a record, an `option` or a tuple is
  carried by the same walk — `bindgen/tests/generate.rs` covers those shapes,
  and no example runs one against the real backend yet.
- **Every call re-runs `main_function`**, as in `plus1`: the fix is the
  [heap reset](../../TODO.md#heap-reset), and the cost is calls rather than
  correctness.
- **One `unreachable`** remains in the adapter, for a memory with no room for
  the 52-byte reservation at the bottom of a fresh heap. A failure with nowhere
  to write cannot be reported through a return area; nothing else here traps.
