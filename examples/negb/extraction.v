(* What `componentize-formal generate` emits from wit/world.wit and
 * component.toml. *)
From CertiRocq.Plugin Require Import CertiRocq.
Require Import impl.

CertiRocq Compile Wasm --output "rocq-negb" Impl.negb.
