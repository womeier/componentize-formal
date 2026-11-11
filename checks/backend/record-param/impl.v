From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition is_empty (s : span) : bool := (byte_length s =? 0)%uint63.
End Impl.
