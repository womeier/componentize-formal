//! The two Rocq files: the signature the implementation is checked against,
//! and the compile invocation.

use std::fmt::Write;

use super::comment;
use super::plan::{Plan, Repr};
use super::MARKER;

/// `interface.v`: the world as a `Module Type`.
///
/// This file is what makes a mismatch a type error. The implementation is
/// written `Module Impl <: WrapperWorld`, so `rocq c` checks it against the
/// world's signature before anything is compiled to wasm -- the one check in
/// the pipeline that is Rocq's rather than this tool's.
pub fn interface(plan: &Plan) -> String {
    let e = &plan.export;
    let mut out = comment::rocq(&[
        format!("{MARKER} wrote this from the world `{}`.", plan.world_name),
        String::new(),
        "The world's signature as a Module Type, so `rocq c` checks your".into(),
        "implementation against it: a mismatch is a type error in your file".into(),
        "rather than a component that misbehaves at run time.".into(),
        String::new(),
        format!(
            "Note what is absent. `{}` returns {} in the",
            e.wit_name,
            show_result(plan)
        ),
        format!(
            "world and `{}` here. The ways the call can fail --",
            rocq_signature(plan)
        ),
        format!("{} -- all belong to the adapter, and", failures(plan)),
        "a Rocq function has no way to produce any of them.".into(),
    ]);

    if plan.uses_int63() {
        out.push_str("From Stdlib Require Import Uint63.\n");
    }

    // Declared outside the Module Type on purpose: a type the world defines
    // belongs to the interface, but an inductive declared *inside* a module
    // type has to be restated by every module implementing it, and the user's
    // file should not have to repeat what this one already says.
    for ind in &plan.inductives {
        writeln!(out, "\n(* {} *)", ind.wit_declaration()).unwrap();
        writeln!(
            out,
            "Inductive {} :=\n{}.",
            ind.rocq_name,
            ind.constructors
                .iter()
                .map(|c| format!("  | {c}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
        .unwrap();
        out.push_str(&comment::rocq(&[
            "Constructor order is load-bearing twice over: it is this",
            "inductive's ordinal order on the Rocq side and the discriminant",
            "order in the component's type, and the adapter is emitted from the",
            "two agreeing.",
        ]));
    }

    writeln!(out, "\nModule Type {}.", plan.module_type).unwrap();
    writeln!(out, "  (* {} *)", e.signature).unwrap();
    writeln!(
        out,
        "  Parameter {} : {}.",
        e.rocq_name,
        rocq_signature(plan)
    )
    .unwrap();
    writeln!(out, "End {}.", plan.module_type).unwrap();
    out
}

/// `extraction.v`: the `CertiRocq Compile Wasm` invocation.
pub fn extraction(plan: &Plan) -> String {
    let b = &plan.build;
    let mut out = comment::rocq(&[
        format!(
            "{MARKER} wrote this from the world `{}` and",
            plan.world_name
        ),
        "component.toml.".into(),
        String::new(),
        "`--output` is the name the generated build rules expect the backend to".into(),
        "write, so changing it here alone breaks the build rather than".into(),
        "renaming anything.".into(),
    ]);

    out.push_str("From CertiRocq.Plugin Require Import CertiRocq.\n");
    writeln!(out, "Require Import {}.", b.impl_stem).unwrap();

    let flags = match b.flags.is_empty() {
        true => String::new(),
        false => format!("{} ", b.flags.join(" ")),
    };
    writeln!(
        out,
        "\nCertiRocq Compile Wasm {flags}--output \"{}\" {}.{}.",
        b.wasm_name, b.impl_module, plan.export.rocq_name
    )
    .unwrap();
    out
}

/// What the adapter can report, as the gate lists it. Taken from there rather
/// than restated, so a file that mentions a failure is a file whose adapter
/// has a branch for it.
fn failures(plan: &Plan) -> String {
    plan.export
        .failures
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// `f : int -> int`, the Rocq side of the export.
fn rocq_signature(plan: &Plan) -> String {
    format!(
        "{} -> {}",
        plan.export.param.repr.rocq_type(),
        plan.export.ok.rocq_type()
    )
}

/// `result<u64, error>`, as the world writes the return type.
fn show_result(plan: &Plan) -> String {
    plan.export
        .signature
        .rsplit_once("-> ")
        .map(|(_, result)| result.to_string())
        .unwrap_or_else(|| "the result".into())
}

impl Plan {
    /// Does any carried type need Rocq's primitive integers?
    fn uses_int63(&self) -> bool {
        [&self.export.param.repr, &self.export.ok]
            .iter()
            .any(|r| **r == Repr::Int63)
    }
}
