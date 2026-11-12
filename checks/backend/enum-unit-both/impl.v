Require Import interface.

Module Impl <: WrapperWorld.
  Definition pick (p : both) : both := mk_both only only.
End Impl.
