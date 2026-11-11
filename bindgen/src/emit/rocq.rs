//! The two Rocq files: the signature the implementation is checked against,
//! and the compile invocation.

use std::fmt::Write;

use super::comment;
use super::plan::{Plan, Repr, Sum};
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

    // After every inductive, so a record with an `enum` field refers to a
    // type already in scope. A record is a one-constructor inductive, so the
    // representation is an ordinary boxed constructor -- and declaring it as
    // a `Record` rather than as an `Inductive` is what gives the user the
    // projections, which is the whole reason the field names are carried
    // this far.
    for rec in &plan.records {
        writeln!(out, "\n(* {} *)", rec.wit_declaration()).unwrap();
        // `{ }` with nothing in it for a record with no fields, which is a
        // type Rocq accepts and whose constructor is nullary -- so the value
        // is an immediate rather than a block (repr.rs SINGLE_CTOR_ORDINAL).
        let fields = rec
            .fields
            .iter()
            .map(|(name, ty)| format!("{name} : {ty}"))
            .collect::<Vec<_>>()
            .join(" ; ");
        let body = match fields.is_empty() {
            true => "{ }".to_string(),
            false => format!("{{ {fields} }}"),
        };
        writeln!(out, "Record {} := {} {body}.", rec.rocq_name, rec.ctor).unwrap();
        out.push_str(&comment::rocq(&[
            "Field order is load-bearing: it is the order the canonical ABI",
            "lays the fields out in and the order the constructor takes them",
            "in, and the adapter is emitted from the two agreeing. The names",
            "are projections, so `start s` is how the field is read.",
        ]));
    }

    writeln!(out, "\nModule Type {}.", plan.module_type).unwrap();
    writeln!(out, "  (* {} *)", e.signature).unwrap();
    // Which constructor is which case. The mapping is not guessable from the
    // names -- the ok half of a `result<T>` is `Some` and the ok half of a
    // `result<_, E>` is `None` -- and nothing else in the pipeline would say
    // so: a file that gets it the wrong way round type-checks and returns the
    // other case.
    for sum in sums(plan) {
        writeln!(
            out,
            "  (* `{}` is carried as `{}`: {} *)",
            sum.wit,
            sum.rocq_type,
            sum.cases
                .iter()
                .map(|c| format!("`{}` is `{}`", c.wit_name, c.ctor))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .unwrap();
    }
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
///
/// One arrow per WIT parameter, in declaration order. Curried rather than a
/// tuple because that is what the compiled term is: the adapter applies the
/// arguments one at a time (TODO.md#arity), so a `Module Type` promising
/// `int * int -> int` would type-check against an implementation the adapter
/// cannot call.
fn rocq_signature(plan: &Plan) -> String {
    plan.export
        .params
        .iter()
        .map(|p| p.repr.rocq_type())
        .chain([plan.export.ok.rocq_type()])
        .collect::<Vec<_>>()
        .join(" -> ")
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
    ///
    /// Recursive, because the payload of an `option` needs the import as much
    /// as a bare parameter does -- and a signature mentioning `int` without
    /// it does not compile, which is a failure the user would have to read
    /// Rocq's output to understand.
    fn uses_int63(&self) -> bool {
        self.carried().any(Repr::carries_int63)
    }

    /// Every type the signature carries, parameters then result.
    fn carried(&self) -> impl Iterator<Item = &Repr> {
        self.export
            .params
            .iter()
            .map(|p| &p.repr)
            .chain([&self.export.ok])
    }
}

/// Every sum in the signature, outermost first and deduplicated by how it
/// reads in WIT.
fn sums(plan: &Plan) -> Vec<&Sum> {
    fn walk<'a>(repr: &'a Repr, found: &mut Vec<&'a Sum>) {
        match repr {
            Repr::Sum(sum) => {
                if !found.iter().any(|seen| seen.wit == sum.wit) {
                    found.push(sum);
                }
                for case in &sum.cases {
                    if let Some(payload) = &case.payload {
                        walk(payload, found);
                    }
                }
            }
            // A field can be an `option` as much as a parameter can, and the
            // comment is as unguessable there: `record cursor { at: option<u64> }`
            // needs the reader to know that `none` is `None`.
            Repr::Record(rec) => {
                for field in &rec.fields {
                    walk(&field.repr, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for repr in plan.carried() {
        walk(repr, &mut found);
    }
    found
}
