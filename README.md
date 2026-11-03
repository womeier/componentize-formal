# CertiRocq components

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

Today only the support gate is implemented:

```
$ nix develop
$ cd bindgen && cargo run -- check --wit ../../testing/examples/component/plus1/world.wit
warning: export plus1: param `val`: u64 is carried by Uint63.int, so values >= 2^63 ...
ok: world `wrapper-world` is within the supported subset
```

It decides whether a world is within the subset the backend can honour and
reports precisely what it cannot, pointing at the matching section of
[`TODO.md`](TODO.md). Correctness is the point — see
[no silent failure](TODO.md#no-silent-failure).

## What works

The generator itself is still being built. Only the support gate is
implemented — it decides whether a world is within the subset the backend can
honour and reports precisely what it cannot.

| | Status |
| --- | --- |
| `check` — accept or reject a world, with reasons | **supported** |
| `Iface.v`, `extraction.v`, `adapter.wat`, build rules | planned (phase 1) |

## WIT feature support

Everything marked *planned* or *not planned* is **rejected with a reason and a
pointer into [`TODO.md`](TODO.md)**, never silently mishandled.

### Types

| Feature | Status | Notes |
| --- | --- | --- |
| `bool` | **supported** | tagged immediate, `false`=1, `true`=3 |
| `u64` | **supported** | Rocq's primitive 63-bit ints (`Uint63.int`); values ≥ 2⁶³ don't fit — warned at the position, guarded in the adapter |
| `u32` | [not planned](TODO.md#integer-widths) | a narrower width needs a range check on every lift |
| `enum` | **supported** | all-nullary inductive, `2*ord+1` |
| `option<T>`, `result<T, E>` as the result | **supported** | doubles as the out-of-memory channel |
| type aliases | **supported** | looked through |
| `record`, `tuple`, `variant` | planned (phase 3) | needs split ordinal mapping + canonical-ABI layout |
| nested `option`/`result` | planned (phase 3) | same machinery |
| `s8`, `s16`, `s32`, `s64` | [wanted, unscheduled](TODO.md#signed-integers) | `Sint63` vs two's-complement `Uint63` undecided |
| `f64` | planned (phase 4) | boxed as primitive ints are, on the backend's `wasm-prim-floats` branch |
| `f32` | [not planned](TODO.md#f32) | `PrimFloat` is binary64 only; nothing carries an `f32` faithfully |
| `list<T>`, fixed-length lists, `map<K, V>` | [not planned](TODO.md#lists) | needs `cabi_realloc` + cons-cell walking |
| `string` | [not planned](TODO.md#strings) | a list plus an encoding choice |
| `char` | [not planned](TODO.md#char) | no faithful Rocq counterpart |
| `flags` | [not planned](TODO.md#flags) | a bitfield with no Rocq counterpart |
| `resource`, `own<T>`, `borrow<T>` | [not planned](TODO.md#resources) | needs handle tables and destructors; there is no GC |
| `future<T>`, `stream<T>`, `error-context` | [not planned](TODO.md#async) | the backend is synchronous |

### Worlds and signatures

| Feature | Status | Notes |
| --- | --- | --- |
| one `export name: func(x: T) -> U` | **supported** | the shape `plus1` and `negb` already use |
| several parameters | planned (phase 2) | iterated closure application |
| no parameters, or no result | planned (phase 2) | different adapter prologue |
| several exports | planned (phase 2) | one compiled `MkExports` term carrying every closure |
| imports | [not planned](TODO.md#imports) | `mod_imports := []` is baked into the correctness proof |
| `export some:pkg/iface` | [not planned](TODO.md#exported-interfaces) | only top-level function exports |
| verified glue | [research project](TODO.md#verified-glue) | the adapter is trusted, as the C glue is |

## Layout

| Path | |
| --- | --- |
| [`bindgen/`](bindgen) | the generator, Rust, on `wit-parser` |
| [`bindgen/src/support.rs`](bindgen/src/support.rs) | the support gate |
| [`bindgen/tests/wit/{good,bad}`](bindgen/tests/wit) | one fixture per accepted and rejected construct |
| [`TODO.md`](TODO.md) | what is deferred, why, and what it would cost |

`nix develop` provides Rocq, CertiRocq, `wasm-tools`, `binaryen` (`wasm-merge`),
`wasmtime`, and a Rust toolchain. `nix build .#bindgen` builds the generator.
