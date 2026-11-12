(* componentize-formal generate wrote this from the world `wrapper-world` and
 * component.toml.
 *
 * `--output` is the name the generated build rules expect the backend to
 * write, so changing it here alone breaks the build rather than
 * renaming anything. *)
From CertiRocq.Plugin Require Import CertiRocq.
Require Import impl.

CertiRocq Compile Wasm --output "rocq-grow" Impl.grow.
