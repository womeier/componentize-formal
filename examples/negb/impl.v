(* Your file. *)
Require Import Iface.

Module Impl <: WrapperWorld.
  Definition negb (b : bool) : bool := if b then false else true.
End Impl.
