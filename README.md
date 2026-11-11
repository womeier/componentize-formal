# componentize-formal

[![CI](https://github.com/womeier/componentize-formal/actions/workflows/ci.yml/badge.svg)](https://github.com/womeier/componentize-formal/actions/workflows/ci.yml)

This project is a WIP. Don't expect a stable CLI interface.

## Overview
This tool allows developing formally-verified [Wasm components](https://component-model.bytecodealliance.org/) in Rocq.
What [ComponentizeJS](https://github.com/bytecodealliance/componentize-js) is to JavaScript, this tool is for theorem provers like Rocq or Lean.
It requires a `.wit` file describing the Wasm component interface, for which it generates a project scaffolding and build setup.
Support for Lean and Agda may be added in the future, since they are supported [Peregrine](https://peregrine-project.github.io/) frontends.

## Verification status
The extraction from Rocq to a Wasm module is verified.
All other steps in turning that module into a component are not yet verified.
The intention is that there should be no crashes or silent failures.

## Quickstart
See [`examples/plus1`](examples/plus1) for the whole pipeline end to end, including the
hand-written glue the generator is checked against.

## How to use
Given `world.wit`, the generator emits:
- `interface.v`: a `Module Type` carrying the WIT signature as Rocq types
- `extraction.v`: the `CertiRocq Compile Wasm` invocation, with your flags
- `adapter.wat`: glue bridging the backend's value representation and the component model's canonical ABI;
- `Makefile`: to build the component

You write one file: `impl.v`, and `generate` writes the project:
```
$ componentize-formal generate --wit <file> --out-dir .
  interface.v                        the world as a Rocq Module Type
  extraction.v                       the CertiRocq Compile Wasm invocation
  adapter.wat                        canonical-ABI glue
  Makefile                           build rules: rocq c, wasm-merge, component embed, component new
  wit/deps/certirocq-abi/errors.wit  the shipped error enum, where `component embed` will look for it

Write impl.v implementing `Module Type WrapperWorld`, then `make` in .
```

### Result type `result<T, error>`
**All** exported functions need to return this exact `result<T, error>` type. (see the [`error` type](bindgen/wit/certirocq-abi.wit))
For instance, in the following function the `u64` is mapped to Rocq's `UPrim63`, which can fail if the input number doesn't fit.
```wit
package local:demo;

world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export plus1: func(val: u64) -> result<u64, error>;
}
```

## WIT feature support
The following features are currently supported.

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
| `string` | [not supported](TODO.md#strings) | brings `cabi_realloc`, requires PrimString |
| `char` | [not planned](TODO.md#char) | no faithful Rocq counterpart |
| `flags` | [not planned](TODO.md#flags) | a bitfield with no Rocq counterpart |
| `resource`, `own<T>`, `borrow<T>` | [not planned](TODO.md#resources) | needs handle tables and destructors; there is no GC |
| `future<T>`, `stream<T>`, `error-context` | [wanted, not scheduled](TODO.md#async) | the backend is synchronous; faithful support is blocked on WasmCert |

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
| Agda | future work | same route through λ□, but two frontends is the commitment |
| verified glue | [not supported](TODO.md#verified-glue) | the adapter is trusted |

## Testing
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
