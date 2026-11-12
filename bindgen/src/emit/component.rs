//! `component.wat`: the component that is itself the link.
//!
//! Where the build used to merge the backend and the adapter into one module
//! and wrap the result, the component now holds both as two core modules and
//! instantiates the adapter with the backend's instance. Imports resolve by
//! instantiation -- the semantics the component model already has -- so
//! neither module is rewritten on the way in, and the proof-carrying backend
//! reaches the component exactly as `rocq c` wrote it
//! (TODO.md#verified-glue).
//!
//! The two core modules appear here as placeholder lines the generated build
//! rules splice over: the backend does not exist until the Rocq half has
//! compiled, and the adapter stays in `adapter.wat` as the one reviewable
//! copy rather than being quoted here a second time.
//!
//! Everything else is the world's type, written out: the `certirocq:abi`
//! error import, a type import per world-declared type, and the lifted
//! export. `wasm-tools component embed` used to derive all of this from the
//! WIT directory; now it is written from the same [`Plan`], and
//! `checks/smoke.py` holds the result to the same answers.
//!
//! Identifier hygiene: ids invented here carry a `.` (`$ret.ty`,
//! `$error.def`), which a WIT identifier cannot contain, while ids standing
//! for something the world named are that name (`$span`). The two sets
//! cannot collide, whatever the world calls its types.

use std::fmt::Write;

use super::plan::{Plan, Record, Repr};
use super::{comment, ADAPTER_MODULE, BACKEND_MODULE, MARKER};
use crate::repr::exports;

pub fn component(plan: &Plan) -> String {
    let e = &plan.export;

    let mut out = comment::wat(
        "",
        &[
            format!("{MARKER} wrote this from the world `{}`:", plan.world_name),
            String::new(),
            format!("    {}", e.signature),
            String::new(),
            "The component *is* the link. CertiRocq's module and the adapter go".into(),
            "in as two core modules; the adapter's imports are satisfied by".into(),
            "instantiating it with the backend's instance. Neither module is".into(),
            "rewritten on the way in -- what `rocq c` emitted is what runs.".into(),
        ],
    );
    out.push_str("(component\n");

    out.push_str(&comment::wat(
        "  ",
        &[
            "The build rules splice `wasm-tools print` of the module `rocq c`".to_string(),
            format!("emits in here, re-headed as `(core module ${BACKEND_MODULE}`. It is the one"),
            "part of this file that does not exist until the Rocq half has".into(),
            "compiled.".into(),
        ],
    ));
    writeln!(out, "  ;; @BACKEND@").unwrap();

    // ---- the error type ----------------------------------------------------
    out.push('\n');
    out.push_str(&comment::wat(
        "  ",
        &[
            format!(
                "`use {}.{{{}}}`: the enum every export reports",
                crate::abi_use_path(),
                crate::ABI_ERROR
            ),
            "its failures through, imported as a type so the component's own type".into(),
            "names the package it came from (TODO.md#abi-package-version). The".into(),
            "case order *is* the discriminant the adapter stores.".into(),
        ],
    ));
    let cases = crate::abi_error_cases()
        .iter()
        .map(|case| format!("\"{case}\""))
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(
        out,
        "  (type $abi.ty (instance\n    \
             (type $e (enum {cases}))\n    \
             (export \"{error}\" (type (eq $e)))))\n  \
         (import \"{path}\" (instance $abi (type $abi.ty)))\n  \
         (alias export $abi \"{error}\" (type $error.def))\n  \
         (import \"{error}\" (type $error (eq $error.def)))",
        error = crate::ABI_ERROR,
        path = crate::abi_use_path(),
    )
    .unwrap();

    // ---- the world's own named types ---------------------------------------
    // Inductives first, then records, which is also how `plan` orders them:
    // a record's fields only ever point backwards.
    //
    // The error enum used as a *value* (a `result<_, error>` parameter, say)
    // gets an inductive declared for it like any other enum, but its type is
    // already imported above -- a world cannot name a second type `error`
    // next to the `use`d one -- so it is the one inductive not declared
    // again here. [`ty`] spells it `$error`, which is the import.
    for inductive in &plan.inductives {
        if inductive.wit_name == crate::ABI_ERROR {
            continue;
        }
        out.push('\n');
        out.push_str(&named_type_comment(&inductive.wit_declaration()));
        let cases = inductive
            .wit_cases
            .iter()
            .map(|case| format!("\"{case}\""))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&named_type(&inductive.wit_name, &format!("(enum {cases})")));
    }
    let used = used_records(plan);
    for decl in &plan.records {
        out.push('\n');
        out.push_str(&named_type_comment(&decl.wit_declaration()));
        let record = used
            .iter()
            .find(|record| record.wit == decl.wit_name)
            .expect("every declared record is somewhere in the export's signature");
        let fields = record
            .fields
            .iter()
            .map(|field| format!("(field \"{}\" {})", field.wit_name, ty(&field.repr, plan)))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&named_type(&decl.wit_name, &format!("(record {fields})")));
    }

    // ---- the two core instances --------------------------------------------
    out.push('\n');
    out.push_str(&comment::wat(
        "  ",
        &[
            "The adapter, spliced in from adapter.wat -- the canonical-ABI glue,".to_string(),
            format!("re-headed as `(core module ${ADAPTER_MODULE}`."),
        ],
    ));
    writeln!(
        out,
        "  ;; @ADAPTER@\n  \
         (core instance $r (instantiate ${backend}))\n  \
         (core instance $a (instantiate ${adapter} (with \"{backend}\" (instance $r))))",
        backend = BACKEND_MODULE,
        adapter = ADAPTER_MODULE,
    )
    .unwrap();

    out.push('\n');
    out.push_str(&comment::wat(
        "  ",
        &[
            "The result is more flat values than the canonical ABI returns, so",
            "the export hands back a pointer and the host reads the value out of",
            "the backend's memory -- the same memory the adapter took the return",
            "area from.",
        ],
    ));
    writeln!(
        out,
        "  (alias core export $r \"{memory}\" (core memory $memory))",
        memory = exports::MEMORY,
    )
    .unwrap();

    // ---- the export, lifted --------------------------------------------------
    let params = e
        .params
        .iter()
        .map(|param| format!("(param \"{}\" {})", param.wit_name, ty(&param.repr, plan)))
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(
        out,
        "\n  (type $ret.ty (result {ok} (error $error)))\n  \
         (type $sig.ty (func {params} (result $ret.ty)))\n  \
         (alias core export $a \"{name}\" (core func ${name}.core))\n  \
         (func ${name} (type $sig.ty) (canon lift (core func ${name}.core) (memory $memory)))\n  \
         (export \"{name}\" (func ${name}))",
        ok = ty(&e.ok, plan),
        name = e.wit_name,
    )
    .unwrap();

    out.push_str(")\n");
    out
}

/// The comment above a world-declared type's import, quoting the declaration
/// the same way `interface.v` does, so the two files tie the name together
/// identically.
fn named_type_comment(declaration: &str) -> String {
    comment::wat(
        "  ",
        &[
            format!("`{declaration}`: declared by the world, so imported as a"),
            "type -- the component's type carries the world's name for it.".into(),
        ],
    )
}

/// A world-named type: the definition, and the import that gives it the
/// world's name. The import is the id the rest of the file refers to, so the
/// component's type says where the type came from.
fn named_type(name: &str, definition: &str) -> String {
    format!(
        "  (type ${name}.def {definition})\n  \
         (import \"{name}\" (type ${name} (eq ${name}.def)))\n"
    )
}

/// A type as the component text format spells it: primitives by name, a
/// world-named type as a reference to its import, an anonymous sum spelled
/// out.
fn ty(repr: &Repr, plan: &Plan) -> String {
    match repr {
        Repr::Bool => "bool".into(),
        Repr::Int63 => "u64".into(),
        Repr::Enum { rocq_type, .. } => {
            let inductive = plan
                .inductives
                .iter()
                .find(|inductive| &inductive.rocq_name == rocq_type)
                .expect("every Enum repr is a world-declared enum the plan recorded");
            format!("${}", inductive.wit_name)
        }
        Repr::Record(record) => format!("${}", record.wit),
        // The four spellings of a two-case sum, told apart the same way
        // `plan` built them: by what discriminant 0 is called.
        Repr::Sum(sum) => match (
            sum.cases[0].wit_name,
            &sum.cases[0].payload,
            &sum.cases[1].payload,
        ) {
            ("none", _, Some(some)) => format!("(option {})", ty(some, plan)),
            ("ok", Some(ok), Some(err)) => {
                format!("(result {} (error {}))", ty(ok, plan), ty(err, plan))
            }
            ("ok", Some(ok), None) => format!("(result {})", ty(ok, plan)),
            ("ok", None, Some(err)) => format!("(result (error {}))", ty(err, plan)),
            _ => unreachable!("a two-case sum is an option or a result, and one case carries"),
        },
    }
}

/// Every `Record` repr in the export's signature, innermost first, so a
/// record declaration can always be rendered from the repr that carries its
/// layout. `plan` orders [`Plan::records`] the same way and from the same
/// walk, which is what makes the lookup by name total.
fn used_records(plan: &Plan) -> Vec<&Record> {
    let mut out = Vec::new();
    for param in &plan.export.params {
        collect(&param.repr, &mut out);
    }
    collect(&plan.export.ok, &mut out);
    out
}

fn collect<'a>(repr: &'a Repr, out: &mut Vec<&'a Record>) {
    match repr {
        Repr::Record(record) => {
            for field in &record.fields {
                collect(&field.repr, out);
            }
            out.push(record);
        }
        Repr::Sum(sum) => {
            for case in &sum.cases {
                if let Some(payload) = &case.payload {
                    collect(payload, out);
                }
            }
        }
        Repr::Bool | Repr::Int63 | Repr::Enum { .. } => {}
    }
}
