# `plus1`

The whole pipeline for the one export shape the gate accepts today:

```wit
export plus1: func(val: u64) -> result<u64, error>;
```

Everything here except `impl.v` is hand-written in the shape
`componentize-formal generate` has to emit, so that generation has a target
known to work rather than a guess. It runs:

```console
$ nix develop
$ cd examples/plus1 && make run-stub
plus1(0) = ok(1)
plus1(8) = ok(9)
plus1(4611686018427387903) = ok(4611686018427387904)
plus1(9223372036854775807) = ok(0)
plus1(9223372036854775808) = err(out-of-range)
$ make run-stub-oom
plus1(8) with the heap exhausted = err(out-of-memory)
```

Two of those lines are the point. `plus1(2⁶³-1) = ok(0)` is not a bug: Rocq's
`Uint63` wraps, so wrapping is the right answer and the adapter must not
invent a failure. `plus1(2⁶³)` is a failure, because that `u64` has no
`Uint63.int` at all — and it is reported, not approximated.

## The files

| | Who writes it |
| --- | --- |
| [`wit/world.wit`](wit/world.wit) | you — the interface comes first |
| [`wit/deps/certirocq-abi/`](wit/deps/certirocq-abi) | vendored, because `wasm-tools component embed` reads the package off disk |
| [`interface.v`](interface.v) | generated — the world as a `Module Type` |
| [`impl.v`](impl.v) | **you** — the only file that is yours |
| [`extraction.v`](extraction.v) | generated — the `CertiRocq Compile Wasm` call |
| [`adapter.wat`](adapter.wat) | generated — the canonical-ABI glue |
| [`Makefile`](Makefile) | generated — `wasm-merge`, `component embed`, `component new` |
| [`component.toml`](component.toml) | you — what the WIT file cannot say |
| [`stub/`](stub) | neither; see below |

`interface.v` asks for `plus1 : int -> int`. The `error` enum is nowhere in it,
because a Rocq function cannot run out of memory in its own type and nothing in
Rocq knows a value has to fit a WIT width. Both failures belong to the adapter,
which is why every export returns `result<T, error>`.

## What the adapter does

In order, from [`adapter.wat`](adapter.wat):

1. Reserves 24 bytes off `mem_ptr` — 16 for the canonical-ABI return area, 8
   for the boxed argument — **8-aligned**, because the host rejects a
   misaligned return area and `mem_ptr` is only 4-aligned in general. Before
   `main_function` runs, so the heap is still empty and the reservation cannot
   realistically fail.
2. Rejects `val ≥ 2⁶³` with `out-of-range`, before running the program.
3. Calls `main_function`, which builds the closure, and checks `out_of_mem`.
4. Reads `[tag, func_idx, env_ptr]` out of the closure and calls the body
   through the table with the boxed argument.
5. Checks `out_of_mem` again — the call allocated its own result, so this is a
   second place the flag can be set — and lowers either `ok(i64)` or
   `err(case)` into the return area.

`result<u64, error>` is 16 bytes, align 8: a one-byte discriminant at offset 0,
the payload at offset 8. The case order in `world.wit` *is* the discriminant,
which is why the error package is versioned.

## The stub

[`stub/backend.wat`](stub/backend.wat) stands in for the module CertiRocq
emits: the same exports, the same value representation, the same bump allocator
that sets `out_of_mem` instead of trapping. [`stub/oom.wat`](stub/oom.wat) is
the same with a heap too small for the closure, which is the only way to reach
the out-of-memory branch at this size.

What the stub pins is the adapter's half — the lowering, the discriminants, and
that both checks fire. It is **not** evidence about the backend. That needs
`make run`, which compiles `impl.v` with the real CertiRocq:

The Rocq side compiles today. The link step needs the backend to export its
function table, which the released CertiRocq does not do, so `flake.nix`
builds CertiRocq from a branch that adds it
([`TODO.md#table-export`](../../TODO.md#table-export)) — a build this example
has not been run against yet. Against a CertiRocq *without* the export, the
Makefile stops with the reason instead of at link time with a missing symbol:

```console
$ make run
rocq c interface.v
rocq c impl.v
rocq c extraction.v
error: this CertiRocq does not export its function table as
       "table", which the adapter links the compiled closures
       through. Move the `certirocq` flake input forward -- see
       TODO.md#table-export. `make run-stub` exercises the
       adapter without it.
```

Getting past that step is what layer 2 of
[representation conformance](../../TODO.md#representation) needs, and this
example is the shape it will take.

## Known gaps

- **Every call re-runs `main_function`**, so every call re-allocates the
  closure and nothing is ever freed. Deliberate for now: the fix is the
  [heap reset](../../TODO.md#heap-reset), and leaving it out costs calls rather
  than correctness — the limit arrives as `out-of-memory`, not as a wrong
  answer.
- **One `unreachable`** remains, for a memory with no room for a 24-byte return
  area at the bottom of a fresh heap. A failure with nowhere to write cannot be
  reported through a return area; nothing else here traps.
