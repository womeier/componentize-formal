(* Your file. The one thing the generator does not write.
 *
 * Deliberately not symmetric in the two fields: a `grow` that touched both
 * the same way would hide a swapped pair of field offsets, because lifting
 * and lowering with the same mistake cancels out and the answer comes back
 * right. *)
From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  Definition grow (s : span) : span :=
    mk_span (start s) (byte_length s + 1)%uint63.
End Impl.
