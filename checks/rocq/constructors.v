(* Compiled by `checks/representation.py` to read the constructor encoding
 * back off the real backend.
 *
 * Constant functions on purpose. `negb` would hide a swapped encoding
 * completely -- it is an involution, so lifting and lowering with the same
 * wrong mapping cancels out and the answer comes back right. A function that
 * ignores its argument cannot cancel anything.
 *
 * `bool` is the case worth watching: the pipeline swaps its constructors so
 * the pair matches OCaml's `false = 0`, `true = 1`, even though Rocq declares
 * `true` first. `colour` is the control -- three constructors that must come
 * out in declaration order. *)
From CertiRocq.Plugin Require Import CertiRocq.

Inductive colour := red | green | blue.

Definition to_true  (b : bool) := true.
Definition to_false (b : bool) := false.
Definition to_red   (b : bool) := red.
Definition to_green (b : bool) := green.
Definition to_blue  (b : bool) := blue.

CertiRocq Compile Wasm -file "ctor-true"  to_true.
CertiRocq Compile Wasm -file "ctor-false" to_false.
CertiRocq Compile Wasm -file "ctor-red"   to_red.
CertiRocq Compile Wasm -file "ctor-green" to_green.
CertiRocq Compile Wasm -file "ctor-blue"  to_blue.
