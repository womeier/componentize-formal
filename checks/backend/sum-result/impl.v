From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition halve (n : int) : option int :=
    if (2 * (n / 2) =? n)%uint63 then Some (n / 2)%uint63 else None.
End Impl.
