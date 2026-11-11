From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition flip (x : sum int bool) : sum int bool :=
    match x with
    | inl n => inr (n =? 0)%uint63
    | inr b => inl (if b then 1%uint63 else 0%uint63)
    end.
End Impl.
