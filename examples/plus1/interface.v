(* What `componentize-formal generate` emits from wit/world.wit.
 *
 * The world's signature, as a Module Type: `rocq c` checks the
 * implementation against it, so a mismatch is a type error in your file
 * rather than a component that misbehaves at run time.
 *
 * Note what is absent. `plus1` returns `result<u64, error>` in the world, and
 * `int -> int` here: out-of-memory and out-of-range belong to the adapter, and
 * a Rocq function has no way to produce either. *)
From Stdlib Require Import Uint63.

Module Type WrapperWorld.
  (* export plus1: func(val: u64) -> result<u64, error> *)
  Parameter plus1 : int -> int.
End WrapperWorld.
