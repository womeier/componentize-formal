(* Your file. The one thing the generator does not write. *)
From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition minus (a b : int) : int := (a - b)%uint63.
End Impl.
