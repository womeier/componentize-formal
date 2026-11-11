From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition is_zero (n : int) : bool := (n =? 0)%uint63.
End Impl.
