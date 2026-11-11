From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition mix (a b c d e f : int) : int := (a - b + c - d + e - f)%uint63.
End Impl.
