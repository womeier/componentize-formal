Require Import interface.

Module Impl <: WrapperWorld.
  Definition is_green (c : colour) : bool :=
    match c with
    | green => true
    | red | blue => false
    end.
End Impl.
