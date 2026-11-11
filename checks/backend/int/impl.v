(* The implementation. Everything else in the case is generated. *)
From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition succ (n : int) : int := (n + 1)%uint63.
End Impl.
