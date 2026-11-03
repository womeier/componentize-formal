(* Your file. The one thing the generator does not write. *)
From Stdlib Require Import Uint63.
Require Import Iface.

Module Impl <: WrapperWorld.
  Definition plus1 (n : int) : int := (n + 1)%uint63.
End Impl.
