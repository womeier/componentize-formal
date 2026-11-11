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

(* Stdlib's `option` and `sum`: what the generator carries a WIT `option` and
 * `result` in (TODO.md#option-result). These are not declared here, which is
 * the point -- they come out of the standard library through MetaRocq's
 * erasure, so a remapping like `bool`'s would apply to them without anything
 * in this repository mentioning them.
 *
 * `option` is the shape a remapping cannot disturb: `Some` is its only boxed
 * constructor and `None` its only nullary one, so per-class numbering gives
 * each of them ordinal 0 however they are ordered. `sum` is the one to watch
 * -- `inl` and `inr` are both boxed, so they are ordinals 0 and 1 in
 * declaration order, and a swap would silently exchange the ok and err halves
 * of a WIT `result`. *)
Definition to_none (b : bool) : option bool := None.
Definition to_some (b : bool) : option bool := Some b.
Definition to_inl  (b : bool) : bool + bool := inl b.
Definition to_inr  (b : bool) : bool + bool := inr b.

(* The other half of carrying a sum: telling a pointer from an immediate. The
 * backend's own `match` does it with `x & 1`, zero meaning boxed
 * (`LambdaANF_to_Wasm.v:349-356`), and a lift that reads an `option` has to
 * make the same test -- so this is compiled to read the mask back out of it
 * rather than to take the source's word. *)
Definition un_option (o : option bool) : bool :=
  match o with
  | Some b => b
  | None => false
  end.

CertiRocq Compile Wasm --output "ctor-none" to_none.
CertiRocq Compile Wasm --output "ctor-some" to_some.
CertiRocq Compile Wasm --output "ctor-inl"  to_inl.
CertiRocq Compile Wasm --output "ctor-inr"  to_inr.
CertiRocq Compile Wasm --output "ctor-match-option" un_option.
