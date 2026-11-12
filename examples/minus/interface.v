(* componentize-formal generate wrote this from the world `wrapper-world`.
 *
 * The world's signature as a Module Type, so `rocq c` checks your
 * implementation against it: a mismatch is a type error in your file
 * rather than a component that misbehaves at run time.
 *
 * Note what is absent. `minus` returns result<u64, error> in the
 * world and `int -> int -> int` here. The ways the call can fail --
 * out of memory, out of range (`a` >= 2^63), out of range (`b` >= 2^63) -- all belong to the adapter, and
 * a Rocq function has no way to produce any of them. *)
From Stdlib Require Import Uint63.

Module Type WrapperWorld.
  (* export minus: func(a: u64, b: u64) -> result<u64, error> *)
  Parameter minus : int -> int -> int.
End WrapperWorld.
