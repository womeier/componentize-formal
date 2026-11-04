(* The shape `componentize-formal generate` will emit from wit/world.wit once
 * variants are supported. It cannot emit this today: the gate rejects a
 * `variant` with E-STRUCTURED, and this file is what it is being written
 * against (TODO.md#structured-types).
 *
 * A WIT variant becomes an ordinary inductive, one constructor per case, in
 * declaration order -- which is *not* the order of the constructor ordinals
 * the backend assigns. See adapter.wat; that mismatch is the whole point of
 * this example.
 *
 * Note what is absent, as in every generated interface: `add-byte` returns
 * `result<byte-count, error>` in the world and `byte_count -> byte_count`
 * here. `too-big` is not a failure -- it is a case of the user's own type, and
 * the Rocq function decides it. The error channel carries only what the
 * adapter owns: out of memory, and a payload no `Uint63.int` represents. *)
From Stdlib Require Import Uint63.

(* variant byte-count { unknown, exactly(u64), at-least(u64), too-big } *)
Inductive byte_count :=
  | unknown
  | exactly (n : int)
  | at_least (n : int)
  | too_big.

Module Type WrapperWorld.
  (* export add-byte: func(c: byte-count) -> result<byte-count, error> *)
  Parameter add_byte : byte_count -> byte_count.
End WrapperWorld.
