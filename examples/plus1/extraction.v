(* What `componentize-formal generate` emits from wit/world.wit and
 * component.toml: the compile invocation, with the configured flags and the
 * `--output` name the build rules expect. *)
From CertiRocq.Plugin Require Import CertiRocq.
Require Import impl.

CertiRocq Compile Wasm --output "rocq-plus1" Impl.plus1.
