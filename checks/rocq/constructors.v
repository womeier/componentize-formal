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

(* The case the declaration index is the wrong thing to use for.
 * `match_ordinals_with_tag` numbers constructors with independent boxed and
 * unboxed counters, so `m_last` -- declaration index 3 -- is unboxed ordinal
 * 1, and the two boxed constructors are ordinals 0 and 1, reusing numbers the
 * unboxed ones already have. Only the tag bit separates them, and an adapter
 * built on the declaration index names a different constructor while
 * type-checking perfectly. `examples/variant` is the adapter that depends on
 * this. *)
Inductive mixed :=
  | m_first
  | m_boxed_a (b : bool)
  | m_boxed_b (b : bool)
  | m_last.

Definition to_true  (b : bool) := true.
Definition to_false (b : bool) := false.
Definition to_red   (b : bool) := red.
Definition to_green (b : bool) := green.
Definition to_blue  (b : bool) := blue.

Definition to_m_first (b : bool) := m_first.
Definition to_m_last  (b : bool) := m_last.

(* These two are read by diffing the modules they compile to, so they must
 * differ in nothing but the constructor they build. *)
Definition to_m_boxed_a (b : bool) := m_boxed_a b.
Definition to_m_boxed_b (b : bool) := m_boxed_b b.

CertiRocq Compile Wasm --output "ctor-true"  to_true.
CertiRocq Compile Wasm --output "ctor-false" to_false.
CertiRocq Compile Wasm --output "ctor-red"   to_red.
CertiRocq Compile Wasm --output "ctor-green" to_green.
CertiRocq Compile Wasm --output "ctor-blue"  to_blue.

CertiRocq Compile Wasm --output "ctor-mixed-first"   to_m_first.
CertiRocq Compile Wasm --output "ctor-mixed-last"    to_m_last.
CertiRocq Compile Wasm --output "ctor-mixed-boxed-a" to_m_boxed_a.
CertiRocq Compile Wasm --output "ctor-mixed-boxed-b" to_m_boxed_b.
