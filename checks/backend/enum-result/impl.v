From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition colour_of (n : int) : colour :=
    if (n =? 0)%uint63 then red
    else if (n =? 1)%uint63 then green
    else blue.
End Impl.
