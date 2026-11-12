Require Import interface.

Module Impl <: WrapperWorld.
  Definition pick (o : one) : one := only.
End Impl.
