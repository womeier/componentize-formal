# componentize-formal

[![CI](https://github.com/womeier/componentize-formal/actions/workflows/ci.yml/badge.svg)](https://github.com/womeier/componentize-formal/actions/workflows/ci.yml)

Turn a WIT world into a Wasm component whose implementation you write in a
proof assistant, compiled by CertiRocq's verified Wasm backend. Rocq today;
which proof assistant it is affects only two generated files, so
[Lean fits](TODO.md#frontends) without disturbing the rest.

The *formal* in the name is about where the code comes from, not a claim that
the component is verified end to end: the backend's compilation is proved
correct, the canonical-ABI glue this tool generates is
[trusted](TODO.md#verified-glue).

Given `world.wit`, the generator emits

- `interface.v` — a `Module Type` carrying the WIT signature as Rocq types, so Rocq
  checks your implementation against the interface at `rocq c` time;
- `extraction.v` — the `CertiRocq Compile Wasm` invocation, with your flags;
- `adapter.wat` — glue bridging the backend's value representation and the
  component model's canonical ABI;
- a `Makefile` that runs `rocq c`, then `wasm-merge`, then `wasm-tools component
  embed` and `component new`.

You write one file: `impl.v`.

Nothing is generated for a world the backend cannot honour. The support gate
runs first, every time, and reports precisely what it turns away, pointing at
the matching section of [`TODO.md`](TODO.md):

```
$ nix develop
$ cd bindgen && cargo run -- check --wit world.wit
error[E-NO-CHANNEL]: a result type with no error channel is not supported
  --> export plus1: result
  = cannot be compiled: every export must return result<T, error> with
    certirocq:abi/errors@0.1.0.error, because this call can fail with: out of
    memory, out of range (`val` >= 2^63)
  = see TODO.md#failure-channel
```

Fix that, and `generate` writes the project:

```
$ componentize-formal generate --wit wit --out-dir .
  interface.v                        the world as a Rocq Module Type
  extraction.v                       the CertiRocq Compile Wasm invocation
  adapter.wat                        canonical-ABI glue
  Makefile                           build rules: rocq c, wasm-merge, component embed, component new
  wit/deps/certirocq-abi/errors.wit  the shipped error enum, where `component embed` will look for it

Write impl.v implementing `Module Type WrapperWorld`, then `make` in .
```

`impl.v` is yours and is never written. Everything else carries a
`componentize-formal generate` line, and a file without one stops the run
before anything at all is written — `--force` overwrites it anyway. The
vendored error package is written only if it is missing, since it is an input
to the component build rather than output.

Correctness is the point — see [no silent failure](TODO.md#no-silent-failure).

## Every export returns `result<T, error>`

Every call can fail: `main_function` runs the whole program and the adapter
allocates the area it returns through. So a world declares the error type this
repo ships, and the gate rejects a return type that cannot report the failures
the signature can produce:

```wit
package local:demo;

world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export plus1: func(val: u64) -> result<u64, error>;
}
```

`option<u64>` is not enough — `plus1` can fail two ways, out of memory and a
`val` of 2⁶³ or more, and `none` cannot say which. The enum never reaches the
Rocq side: that signature asks you for `plus1 : int -> int`. There is no opt-out
— a component that traps instead of answering is not a component anyone can
use. Details in [`TODO.md#failure-channel`](TODO.md#failure-channel).

## What works

| | Status |
| --- | --- |
| `check` — accept or reject a world, with reasons | **supported** |
| `generate` — `interface.v`, `extraction.v`, `adapter.wat`, `Makefile` | **supported** |
| `roundtrip` — the conversion harness `checks.roundtrip` runs | **supported** |

What holds `generate` to its word is that
[`examples/`](examples) was written, built and run *before* the emitter
existed: `checks.generated` regenerates each example's world and requires the
result to be the hand-written adapter token for token, then assembles the
generated one and requires the same answers from `wasmtime`. The first thing
that check found was a boxed `u64` passed by value instead of by address — a
component that assembles, runs, and answers nonsense.

Those components link against the examples' stubs, which stand in for
CertiRocq's output so the check needs no Rocq. `checks.backend` is the other
half: a generated world per representation compiled with the real CertiRocq,
merged with its generated adapter, and invoked — including one invocation with
the heap capped at the module's declared minimum, where the real allocator
fails and the adapter has to answer `err(out-of-memory)`.

Some examples run *ahead* of the gate. [`variant`](examples/variant) is the
whole pipeline for a construct `generate` still refuses — hand-written,
measured against a module the real CertiRocq compiled, and answering
correctly. It is the target the next support work is written against, and the
check holds the generator to refusing it until that is done.

[`minus`](examples/minus) and [`record`](examples/record) were both there
until several parameters and then records landed, and the move is what that
work was for: the generator now emits their adapters token for token, so the
same hand-written files that were targets are now the reference.

## WIT feature support

Anything not marked **supported** is **rejected with a reason and a pointer
into [`TODO.md`](TODO.md)**, never silently mishandled.

### Types

| Feature | Status | Notes |
| --- | --- | --- |
| `bool` | **supported** | tagged immediate, `false`=1, `true`=3 |
| `u64` | **supported** | Rocq's primitive 63-bit ints (`Uint63.int`); values ≥ 2⁶³ don't fit, so the adapter range-checks the argument and answers `err(out-of-range)` |
| `u8`, `u16`, `u32` | [not supported](TODO.md#integer-widths) | a narrower width needs a range check on every lift |
| `enum` | **supported** | all-nullary inductive, `2*ord+1` |
| `result<T, error>` as the result | **required** | the [failure channel](TODO.md#failure-channel), on every export |
| type aliases | **supported** | looked through |
| `option<T>`, `result<T, E>` as a value | **supported** | Rocq's `option` or `sum`, nested to any depth, with the [case mapping](TODO.md#option-result) written into `interface.v`; a `result` with a payload in neither case is refused |
| `record` | **supported** | one boxed constructor, fields laid out by the canonical ABI, in any position and to any depth; [`examples/record`](examples/record) is the reference. A record with [no fields](TODO.md#record-empty) has no component type, and one with [a single field](TODO.md#record-unboxed) the backend unboxes, so both are refused |
| `tuple` | [not supported](TODO.md#structured-types) | lays out as the equivalent `record` does; what is missing is a name to declare it under in Rocq |
| `variant` | [not supported](TODO.md#structured-types) | needs the backend's split ordinal mapping ported; [`examples/variant`](examples/variant) is the hand-written target, and it runs |
| `s8`, `s16`, `s32`, `s64` | [not supported](TODO.md#signed-integers) | `Sint63` vs two's-complement `Uint63` undecided |
| `f64` | [not supported](TODO.md#f64) | boxed as primitive ints are, on the backend's `wasm-prim-floats` branch |
| `f32` | [not planned](TODO.md#f32) | `PrimFloat` is binary64 only; nothing carries an `f32` faithfully |
| `list<T>`, fixed-length lists | [not supported](TODO.md#lists) | the one recursive former WIT has; shares machinery with strings |
| `map<K, V>` | [not planned](TODO.md#maps) | needs a key ordering with no canonical Rocq map to follow |
| `string` | [not supported](TODO.md#strings) | brings `cabi_realloc` and value walking with it |
| `char` | [not planned](TODO.md#char) | no faithful Rocq counterpart |
| `flags` | [not planned](TODO.md#flags) | a bitfield with no Rocq counterpart |
| `resource`, `own<T>`, `borrow<T>` | [not planned](TODO.md#resources) | needs handle tables and destructors; there is no GC |
| `future<T>`, `stream<T>`, `error-context` | [not planned](TODO.md#async) | the backend is synchronous |

### Worlds and signatures

| Feature | Status | Notes |
| --- | --- | --- |
| one `export name: func(x: T, ...) -> result<U, error>` | **supported** | any number of arguments, one result, one error channel |
| `use` of a type-only interface | **supported** | needs no core import, so it costs nothing |
| several parameters | **supported** | the backend curries, so the adapter applies them one at a time: a box and a `call_indirect` per argument, with the next closure read out of `result` in between ([`examples/minus`](examples/minus)) |
| no parameters, or no result | [not supported](TODO.md#arity) | different adapter prologue |
| several exports | [not supported](TODO.md#multi-export) | one compiled `MkExports` term carrying every closure |
| imports | [not supported](TODO.md#imports) | `mod_imports := []` is baked into the correctness proof |
| `export some:pkg/iface` | [not planned](TODO.md#exported-interfaces) | only top-level function exports |
| Rocq as the source language | **supported** | the default, and the only one implemented |
| Lean | [not supported](TODO.md#frontends) | selectable (`--frontend lean`) and rejected with a reason; the unaffiliated [Peregrine project](https://peregrine-project.github.io/) has a Lean frontend into λ□ feeding this same backend, so only the interface file and the compile driver would differ |
| Agda | [not planned](TODO.md#agda) | same route through λ□, but two frontends is the commitment |
| verified glue | [not supported](TODO.md#verified-glue) | the adapter is trusted, as the C glue is |

## Layout

| Path | |
| --- | --- |
| [`bindgen/`](bindgen) | the generator, Rust, on `wit-parser` |
| [`bindgen/src/support.rs`](bindgen/src/support.rs) | the support gate |
| [`bindgen/src/repr.rs`](bindgen/src/repr.rs) | CertiRocq's value representation, with sources |
| [`bindgen/src/emit/plan.rs`](bindgen/src/emit/plan.rs) | everything the output depends on, decided once — the only part that can fail |
| [`bindgen/src/emit/`](bindgen/src/emit) | the emitters: `rocq.rs`, `adapter.rs`, `build.rs`, each a pure function of the plan |
| [`bindgen/wit/certirocq-abi.wit`](bindgen/wit/certirocq-abi.wit) | the shipped `error` enum every export returns |
| [`bindgen/tests/wit/{good,bad}`](bindgen/tests/wit) | one fixture per accepted and rejected construct |
| [`examples/`](examples) | the whole pipeline for one export, hand-written before the emitter existed and so usable as evidence about it: [`plus1`](examples/plus1), [`negb`](examples/negb), [`minus`](examples/minus), [`record`](examples/record), and [`variant`](examples/variant), which is ahead of the gate |
| [`checks/`](checks) | the checks `nix flake check` runs that aren't `cargo test` |
| [`TODO.md`](TODO.md) | what is deferred, why, and what it would cost |
| [`flake.nix`](flake.nix) | the toolchain, the generator package, and every check CI runs |

## Building

`nix develop` provides Rocq, CertiRocq, `wasm-tools`, `binaryen` (`wasm-merge`),
`wasmtime`, and a Rust toolchain; `nix develop .#bindgen` is the same without
the Rocq package set, for working on the generator alone.
`nix build .#componentize-formal` builds the generator.

`nix flake check` runs the whole gate, and CI does nothing but run that command,
so a green checkout is a green branch:

| Check | |
| --- | --- |
| `componentize-formal` | the generator built and its tests run in the sandbox |
| `clippy`, `fmt` | `clippy -D warnings`, `cargo fmt --check` |
| `abi-version` | `wit-parser`'s version must match the `wasm-tools` that ships, since they must compute the same canonical-ABI layout |
| `representation` | compiles Rocq functions with the real CertiRocq and checks the emitted modules against every claim in `repr.rs`, including the constructor immediates and the independent boxed/unboxed ordinals |
| `backend` | compiles a world per representation with the real CertiRocq, merges it with its generated adapter, and invokes the component: the one check where both halves of the merge are real, and the only place the out-of-memory channel fires on the actual allocator |
| `smoke` | assembles each example into a component and checks what `wasmtime` says it answers |
| `generated` | generates for each example's world and requires the result to be the hand-written files token for token, then assembles the generated adapter and requires the same answers. For an example ahead of the gate, requires instead that the generator refuses it with the right code and writes nothing |
| `roundtrip` | emits each conversion in both orders and runs it under `wasmtime`, requiring the value back unchanged. An export adapter uses each direction in one position only; the other order is the one [function imports](TODO.md#imports) will need, so it is run before there is anything that needs it |

The last six are Python scripts under [`checks/`](checks) and run standalone
in `nix develop` as well, which is often what you want while changing
`repr.rs`. They guard against the same thing: a mismatch there does not break
the build, it makes the adapter read the wrong bytes and return plausible
garbage.

One part of it does break the build.
[`bindgen/src/measured.rs`](bindgen/src/measured.rs) records the constructor
immediates a compiled module actually contained, and `repr.rs` asserts against
them at compile time, so the generator does not build while it disagrees with
the last measurement. `python3 checks/representation.py --update` re-measures,
which is the step where someone decides the change was intended. See
[representation conformance](TODO.md#representation).
