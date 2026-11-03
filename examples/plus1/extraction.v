(* What `componentize-formal generate` emits from wit/world.wit and
 * component.toml: the compile invocation, with the configured flags and the
 * `-file` name the build rules expect. *)
From CertiRocq.Plugin Require Import CertiRocq.
Require Import impl.

CertiRocq Compile Wasm -file "rocq-plus1" Impl.plus1.
