From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition dig (x : option (option int)) : option (option int) :=
    match x with
    | None => Some None
    | Some None => None
    | Some (Some n) => Some (Some (n + 1)%uint63)
    end.
End Impl.
