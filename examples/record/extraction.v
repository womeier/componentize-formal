(* The compile invocation, with the configured flags and the `-file` name the
 * build rules expect -- the shape `componentize-formal generate` will emit
 * from wit/world.wit and component.toml. *)
From CertiRocq.Plugin Require Import CertiRocq.
Require Import impl.

CertiRocq Compile Wasm -file "rocq-grow" Impl.grow.
