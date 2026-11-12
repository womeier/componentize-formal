(* componentize-formal generate wrote this from the world `wrapper-world`.
 *
 * The world's signature as a Module Type, so `rocq c` checks your
 * implementation against it: a mismatch is a type error in your file
 * rather than a component that misbehaves at run time.
 *
 * Note what is absent. `negb` returns result<bool, error> in the
 * world and `bool -> bool` here. The ways the call can fail --
 * out of memory -- all belong to the adapter, and
 * a Rocq function has no way to produce any of them. *)

Module Type WrapperWorld.
  (* export negb: func(b: bool) -> result<bool, error> *)
  Parameter negb : bool -> bool.
End WrapperWorld.
