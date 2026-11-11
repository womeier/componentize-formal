(* What `componentize-formal generate` emits from wit/world.wit.
 *
 * The world's signature, as a Module Type: `rocq c` checks the
 * implementation against it, so a mismatch is a type error in your file
 * rather than a component that misbehaves at run time.
 *
 * Two WIT parameters become two Rocq arrows, in declaration order. That is
 * not a notational choice: the compiled closure takes one argument at a time,
 * and the adapter applies them left to right (TODO.md#arity). *)
From Stdlib Require Import Uint63.

Module Type WrapperWorld.
  (* export minus: func(a: u64, b: u64) -> result<u64, error> *)
  Parameter minus : int -> int -> int.
End WrapperWorld.
