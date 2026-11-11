From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition whole (n : int) : span := mk_span n (n + 1)%uint63.
End Impl.
