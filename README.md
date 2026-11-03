# CertiRocq components

[![CI](https://github.com/womeier/componentize-rocq/actions/workflows/ci.yml/badge.svg)](https://github.com/womeier/componentize-rocq/actions/workflows/ci.yml)

Turn a WIT world into a Wasm component backed by CertiRocq's verified Wasm
backend.

Given `world.wit`, the generator will emit

- `Iface.v` — a `Module Type` carrying the WIT signature as Rocq types, so Rocq
  checks your implementation against the interface at `rocq c` time;
- `extraction.v` — the `CertiRocq Compile Wasm` invocation, with your flags;
- `adapter.wat` — glue bridging the backend's value representation and the
  component model's canonical ABI;
- build rules that run `wasm-merge`, then `wasm-tools component embed` and
  `component new`.

Today only the support gate is implemented. It decides whether a world is
within the subset the backend can honour and reports precisely what it cannot,
pointing at the matching section of [`TODO.md`](TODO.md):

```
$ nix develop
$ cd bindgen && cargo run -- check --wit world.wit
error[E-NO-CHANNEL]: a result type with no error channel is not supported
  --> export plus1: result
  = cannot be compiled: every export must return result<T, error> with
    certirocq:abi/errors.error, because this call can fail with: out of memory,
    out of range (`val` >= 2^63). Pass --oom=trap to trap instead.
  = see TODO.md#failure-channel
```

Correctness is the point — see [no silent failure](TODO.md#no-silent-failure).

## Every export returns `result<T, error>`

Every call can fail: `main_function` runs the whole program and the adapter
allocates the area it returns through. So a world declares the error type this
repo ships, and the gate rejects a return type that cannot report the failures
the signature can produce:

```wit
package local:demo;

world wrapper-world {
  use certirocq:abi/errors.{error};
  export plus1: func(val: u64) -> result<u64, error>;
}
```

`option<u64>` is not enough — `plus1` can fail two ways, out of memory and a
`val` of 2⁶³ or more, and `none` cannot say which. The enum never reaches the
Rocq side: that signature asks you for `plus1 : int -> int`. `--oom=trap` opts
out, accepting a bare return type and trapping instead. Details in
[`TODO.md#failure-channel`](TODO.md#failure-channel).

## What works

| | Status |
| --- | --- |
| `check` — accept or reject a world, with reasons | **supported** |
| `Iface.v`, `extraction.v`, `adapter.wat`, build rules | not yet |

## WIT feature support

Anything not marked **supported** is **rejected with a reason and a pointer
into [`TODO.md`](TODO.md)**, never silently mishandled.

### Types

| Feature | Status | Notes |
| --- | --- | --- |
| `bool` | **supported** | tagged immediate, `false`=1, `true`=3 |
| `u64` | **supported** | Rocq's primitive 63-bit ints (`Uint63.int`); values ≥ 2⁶³ don't fit — warned at the position, guarded in the adapter |
| `u8`, `u16`, `u32` | [not supported](TODO.md#integer-widths) | a narrower width needs a range check on every lift |
| `enum` | **supported** | all-nullary inductive, `2*ord+1` |
| `result<T, error>` as the result | **required** | the [failure channel](TODO.md#failure-channel), on every export |
| type aliases | **supported** | looked through |
| `option`/`result` as a value | [not supported](TODO.md#option-result) | fixed shapes, known ordinals — no layout algorithm needed |
| `record`, `tuple` | [not supported](TODO.md#structured-types) | needs split ordinal mapping + canonical-ABI layout |
| `variant` | [not supported](TODO.md#structured-types) | same machinery |
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
| one `export name: func(x: T) -> result<U, error>` | **supported** | one argument, one result, one error channel |
| `use` of a type-only interface | **supported** | needs no core import, so it costs nothing |
| several parameters | [not supported](TODO.md#arity) | iterated closure application |
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
| [`bindgen/wit/certirocq-abi.wit`](bindgen/wit/certirocq-abi.wit) | the shipped `error` enum every export returns |
| [`bindgen/tests/wit/{good,bad}`](bindgen/tests/wit) | one fixture per accepted and rejected construct |
| [`checks/`](checks) | the checks `nix flake check` runs that aren't `cargo test` |
| [`TODO.md`](TODO.md) | what is deferred, why, and what it would cost |
| [`flake.nix`](flake.nix) | the toolchain, the generator package, and every check CI runs |

## Building

`nix develop` provides Rocq, CertiRocq, `wasm-tools`, `binaryen` (`wasm-merge`),
`wasmtime`, and a Rust toolchain; `nix develop .#bindgen` is the same without
the Rocq package set, for working on the generator alone. `nix build .#bindgen`
builds the generator.

`nix flake check` runs the whole gate, and CI does nothing but run that command,
so a green checkout is a green branch:

| Check | |
| --- | --- |
| `bindgen` | the generator built and its tests run in the sandbox |
| `clippy`, `fmt` | `clippy -D warnings`, `cargo fmt --check` |
| `abi-version` | `wit-parser`'s version must match the `wasm-tools` that ships, since they must compute the same canonical-ABI layout |
| `representation` | compiles a Rocq function with the real CertiRocq and checks the emitted module against every claim in `repr.rs` |

The last two are Python scripts under [`checks/`](checks) and run standalone in
`nix develop` as well, which is often what you want while changing `repr.rs`.
Both guard against the same thing: a mismatch there does not break the build,
it makes the adapter read the wrong bytes and return plausible garbage. See
[representation conformance](TODO.md#representation).
