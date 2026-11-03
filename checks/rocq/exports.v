(* The module shape every generated adapter is written against.
 *
 * `bindgen/src/repr.rs` records what the backend emits -- the export names the
 * adapter links to, the function type indices it calls through, that the module
 * imports nothing and has one memory. Its unit tests pin those constants
 * against the hand-written adapters, which catches a mistake in `repr.rs` but
 * not the backend moving underneath it. So `checks.representation` compiles
 * this file with the real CertiRocq and reads the facts back off the result.
 *
 * `plus1` is deliberately dull: the point is the shape of the module around it,
 * not the function. Checks that need to *run* the module -- the tagged
 * immediates, the field offsets, the out-of-memory channel -- need a generated
 * adapter, and arrive with phase 1 (see TODO.md#representation).
 *)

From CertiRocq.Plugin Require Import CertiRocq.
From Stdlib Require Import Uint63.

Definition plus1 (n : int) := (n + 1)%uint63.

CertiRocq Compile Wasm -file "conformance" plus1.
