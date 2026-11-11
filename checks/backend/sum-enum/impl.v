Require Import interface.

Module Impl <: WrapperWorld.
  Definition shade (c : option colour) : option colour :=
    match c with
    | None => Some red
    | Some red => Some blue
    | Some green => None
    | Some blue => Some green
    end.
End Impl.
