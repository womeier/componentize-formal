From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition sub (a b : int) : int := (a - b)%uint63.
End Impl.
