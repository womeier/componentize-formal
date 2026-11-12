(* componentize-formal generate wrote this from the world `wrapper-world`.
 *
 * The world's signature as a Module Type, so `rocq c` checks your
 * implementation against it: a mismatch is a type error in your file
 * rather than a component that misbehaves at run time.
 *
 * Note what is absent. `grow` returns result<span, error> in the
 * world and `span -> span` here. The ways the call can fail --
 * out of memory, out of range (a `u64` inside `s` >= 2^63) -- all belong to the adapter, and
 * a Rocq function has no way to produce any of them. *)
From Stdlib Require Import Uint63.

(* record span { start: u64, byte-length: u64 } *)
Record span := mk_span { start : int ; byte_length : int }.
(* Field order is load-bearing: it is the order the canonical ABI
 * lays the fields out in and the order the constructor takes them
 * in, and the adapter is emitted from the two agreeing. The names
 * are projections, so `start s` is how the field is read. *)

Module Type WrapperWorld.
  (* export grow: func(s: span) -> result<span, error> *)
  Parameter grow : span -> span.
End WrapperWorld.
