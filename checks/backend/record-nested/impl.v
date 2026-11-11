From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition bump (o : outer) : outer :=
    mk_outer (mk_inner (a (i o) + 1)%uint63 (z (i o) + 2)%uint63) (b o + 10)%uint63.
End Impl.
