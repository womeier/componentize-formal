From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition bump (i : item) : item := mk_item (tag i) (n i + 1)%uint63.
End Impl.
