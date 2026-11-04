(* Your file. The one thing the generator does not write.
 *
 * `too_big` is decided here, not by the adapter. A `u64` that overflows on
 * the way *in* is `err(out-of-range)`, because no value of the Rocq type
 * stands for it; a count that overflows while being added to is a case the
 * world declares, so saying so is the implementation's job. The two are
 * different failures and the interface keeps them apart. *)
From Stdlib Require Import Uint63.
Require Import interface.

Module Impl <: WrapperWorld.
  (* `n + 1 = 0` is the only way a successor wraps in Uint63. *)
  Definition add_byte (c : byte_count) : byte_count :=
    match c with
    | unknown => at_least 1
    | exactly n => if ((n + 1) =? 0)%uint63 then too_big else exactly (n + 1)
    | at_least n => if ((n + 1) =? 0)%uint63 then too_big else at_least (n + 1)
    | too_big => too_big
    end.
End Impl.
