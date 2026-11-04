(* What `componentize-formal generate` emits from wit/world.wit. *)

Module Type WrapperWorld.
  (* export negb: func(b: bool) -> result<bool, error> *)
  Parameter negb : bool -> bool.
End WrapperWorld.
