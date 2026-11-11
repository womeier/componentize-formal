From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition unwrap_or (x : option int) : int :=
    match x with
    | None => 7%uint63
    | Some n => n
    end.
End Impl.
