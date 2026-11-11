From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition to_int (b : bool) : int := if b then 1%uint63 else 0%uint63.
End Impl.
