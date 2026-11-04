(* Your file. *)
Require Import interface.

Module Impl <: WrapperWorld.
  Definition negb (b : bool) : bool := if b then false else true.
End Impl.
