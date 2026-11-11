//! `adapter.wat`: the canonical-ABI glue.
//!
//! Every number in the emitted module comes from one of two places --
//! [`crate::repr`] for CertiRocq's side, `wit_parser::SizeAlign` for the
//! component model's -- and never from a literal here. The two hand-written
//! adapters under `examples/` were written before this emitter existed and are
//! now what it is held to, byte for byte, by `checks/generated.py`.

use std::fmt::Write;

use wit_parser::Int;

use super::plan::{Cells, Discriminants, Export, Plan, Repr, ADAPTER_ALIGN};
use super::{comment, convert, BACKEND_MODULE, MARKER};
use crate::repr;
use crate::repr::exports;

/// Locals the emitted function declares for itself. A parameter named one of
/// these would be a duplicate name in the wat, so [`super::plan`] refuses it.
/// `arg` is in the list because it is the box local of a one-parameter export
/// ([`super::plan::ONE_ARG_LOCAL`]); with several parameters the boxes are
/// named after them, and [`super::plan`] checks those too.
pub const LOCALS: &[&str] = &["ret", "arg", "closure"];

pub fn adapter(plan: &Plan) -> String {
    let e = &plan.export;
    let l = &e.layout;
    let d = &e.discriminants;

    let mut out = comment::wat(
        "",
        &[
            format!("{MARKER} wrote this from the world `{}`:", plan.world_name),
            String::new(),
            format!("    {}", e.signature),
            String::new(),
            "It bridges two value representations. CertiRocq's: a bump allocator,".into(),
            format!(
                "{}-byte cells, nullary constructors as immediates. The canonical",
                repr::CELL
            ),
            "ABI's: a return area with a discriminant and a payload at fixed".into(),
            "offsets. Nothing here is chosen -- the offsets come from".into(),
            "bindgen/src/repr.rs and from the same layout code `wasm-tools".into(),
            "component new` uses, so the two cannot disagree about where a byte".into(),
            "goes.".into(),
        ],
    );
    out.push_str("(module\n");

    // ---- imports -----------------------------------------------------------
    out.push_str(&comment::wat(
        "  ",
        &[
            "The compiled module. `wasm-merge` resolves every one of these",
            "against its exports, so none survives into the component -- which",
            "imports nothing at all. Names: repr.rs `exports`.",
        ],
    ));
    for (name, shape) in [
        (exports::MEMORY, "(memory 1)".to_string()),
        (exports::MEM_PTR, "(global $mem_ptr (mut i32))".into()),
        (exports::RESULT, "(global $result (mut i32))".into()),
        (exports::OUT_OF_MEM, "(global $out_of_mem (mut i32))".into()),
        (exports::MAIN_FUNCTION, "(func $main_function)".into()),
        (exports::TABLE, "(table $funcs 1 funcref)".into()),
    ] {
        writeln!(out, "  (import \"{BACKEND_MODULE}\" \"{name}\" {shape})").unwrap();
    }

    // ---- the closure type --------------------------------------------------
    out.push('\n');
    out.push_str(&comment::wat(
        "  ",
        &[
            "How the backend calls a closure body: (env, arg), with the result".to_string(),
            format!(
                "left in `{}`. Every Rocq function has type [i32; n] -> [] and",
                exports::RESULT
            ),
            format!(
                "the backend builds those types in order, so this is index {} --",
                repr::func_type_index(2)
            ),
            "repr.rs func_type_index.".into(),
        ],
    ));
    out.push_str("  (type $closure (func (param i32 i32)))\n\n");

    // ---- the return area ---------------------------------------------------
    out.push_str(&comment::wat(
        "  ",
        &[
            format!(
                "{} in the canonical ABI: size {}, align {}, a {}-byte",
                result_type(e),
                l.ret_size,
                l.ret_align,
                int_bytes(l.ret_tag)
            ),
            format!(
                "discriminant at offset 0 and the payload at offset {}. `result`",
                l.payload
            ),
            format!(
                "declares ok before err, so ok = {} and err = {}.",
                d.ok, d.err
            ),
        ],
    ));
    // A scalar ok is one store at the payload offset. A sum is a
    // discriminant and a payload of its own, written at offsets inside that
    // one -- so the function takes the Rocq value rather than a lifted core
    // value, and `convert` writes the bytes.
    writeln!(
        out,
        "  (func $ok (param $ret i32) (param $val {ty})\n\
         \x20   ({store_tag} (local.get $ret) (i32.const {ok}))",
        ty = match &e.ok {
            Repr::Sum(_) => "i32",
            ok => convert::core_type(ok),
        },
        store_tag = convert::store_op(l.ret_tag),
        ok = d.ok,
    )
    .unwrap();
    for line in convert::store_canonical(
        &e.ok,
        convert::Base("(local.get $ret)"),
        l.payload,
        "(local.get $val)",
    ) {
        writeln!(out, "    {line}").unwrap();
    }
    // The closing paren of the last statement doubles as the function's.
    out.truncate(out.trim_end().len());
    out.push_str(")\n\n");

    out.push_str(&comment::wat(
        "  ",
        &[
            "The error half is an enum, so its payload is a bare discriminant:".to_string(),
            format!("{}. An enum's case order *is* that", error_cases(d)),
            "discriminant, which is why the package declaring it is versioned".into(),
            "(TODO.md#abi-package-version).".into(),
        ],
    ));
    writeln!(
        out,
        "  (func $err (param $ret i32) (param $case i32)\n\
         \x20   ({store_tag} (local.get $ret) (i32.const {err}))\n\
         \x20   ({store_case} offset={payload} (local.get $ret) (local.get $case)))",
        store_tag = convert::store_op(l.ret_tag),
        err = d.err,
        store_case = convert::store_op(l.err_tag),
        payload = l.payload,
    )
    .unwrap();

    // ---- the export --------------------------------------------------------
    let params = e
        .params
        .iter()
        .flat_map(|p| &p.slots)
        .map(|s| format!("(param ${} {})", s.local, s.wasm))
        .collect::<Vec<_>>()
        .join(" ");
    // A parameter the canonical ABI flattens into several values has nothing
    // in the WIT file naming them, so the mapping is written down here rather
    // than left to be inferred from the order.
    let flattened: Vec<String> = e
        .params
        .iter()
        .filter(|p| p.slots.len() > 1)
        .map(|p| {
            format!(
                "`{}: {}` arrives in {}.",
                p.wit_name,
                p.repr.rocq_type(),
                p.slots
                    .iter()
                    .map(|s| format!("${}", s.local))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect();
    // The blank line before the function comes from the comment when there is
    // one, so that the two spellings are the same shape.
    let lead = match flattened.is_empty() {
        true => "\n",
        false => "",
    };
    if !flattened.is_empty() {
        out.push('\n');
        let mut lines = vec![
            "The canonical ABI flattens a sum into a discriminant and the".to_string(),
            "payload slots its two cases share, outermost first:".into(),
            String::new(),
        ];
        lines.extend(flattened);
        out.push_str(&comment::wat("  ", &lines));
    }
    writeln!(
        out,
        "{lead}  (func (export \"{}\") {params} (result i32)",
        e.wit_name
    )
    .unwrap();
    out.push_str("    (local $ret i32)\n");
    for p in &e.params {
        if let Some(block) = &p.block {
            writeln!(out, "    (local ${} i32)", block.local).unwrap();
        }
    }
    out.push_str("    (local $closure i32)\n");

    reserve(&mut out, e);
    if let Some(case) = d.out_of_range {
        for p in &e.params {
            range_checks(&mut out, p, case);
        }
    }
    for p in &e.params {
        validate_param(&mut out, p);
    }
    call(&mut out, e);
    lift(&mut out, e);

    out.push_str("    (local.get $ret))\n)\n");
    out
}

/// Take the return area, and the cells each argument's lowering needs, off the
/// backend's own bump pointer.
fn reserve(out: &mut String, e: &Export) {
    let l = &e.layout;
    let blocks: Vec<&super::plan::Param> = e.params.iter().filter(|p| p.has_block()).collect();

    // A sum's cells are not a box: its empty case stores nothing and is passed
    // as an immediate, so only some of the values built in there are boxed.
    // The hand-written examples' word is kept where it is still true.
    let (one, many) = match blocks.iter().all(|p| p.repr.boxed()) {
        true => ("the boxed argument", "boxed arguments"),
        false => ("the cells one argument is built in", "argument blocks"),
    };
    let mut lines = vec![
        match blocks.len() {
            0 => "The return area comes off the same bump".to_string(),
            1 => format!("The return area and {one} come off the same bump"),
            n => format!("The return area and the {n} {many} come off the same bump"),
        },
        format!(
            "pointer the backend allocates from, before `{}` runs,",
            exports::MAIN_FUNCTION
        ),
        "so the heap is still empty and the reservation cannot fail in".into(),
        "practice.".into(),
        String::new(),
    ];
    // Why it is all taken up front rather than one box reused per argument.
    if blocks.len() > 1 {
        lines.extend([
            "Every one is reserved here and none is reused: applying an".to_string(),
            "argument captures its cells by address, and a later application".into(),
            "reads back through that pointer (TODO.md#arity).".into(),
            String::new(),
        ]);
    }
    lines.extend([
        format!("{ADAPTER_ALIGN}-aligned because the host rejects a misaligned return area,"),
        format!(
            "and `{}` is only {}-aligned in general (repr.rs:",
            exports::MEM_PTR,
            repr::CELL
        ),
        format!(
            "{}-byte cells); reserving a multiple of {ADAPTER_ALIGN} leaves it where the",
            repr::CELL
        ),
        "backend wants it again.".into(),
    ]);

    out.push('\n');
    out.push_str(&comment::wat("    ", &lines));
    writeln!(
        out,
        "    (global.set $mem_ptr\n\
         \x20     (i32.and (i32.add (global.get $mem_ptr) (i32.const {})) (i32.const {})))",
        ADAPTER_ALIGN - 1,
        -(ADAPTER_ALIGN as i32),
    )
    .unwrap();
    out.push_str("    (local.set $ret (global.get $mem_ptr))\n");
    for p in &e.params {
        if let Some(block) = &p.block {
            writeln!(
                out,
                "    (local.set ${} (i32.add (local.get $ret) (i32.const {})))",
                block.local, block.offset
            )
            .unwrap();
        }
    }
    writeln!(
        out,
        "    (global.set $mem_ptr (i32.add (local.get $ret) (i32.const {})))\n",
        l.reserve
    )
    .unwrap();

    out.push_str(&comment::wat(
        "    ",
        &[
            "Nothing bounds-checks what the adapter takes for itself, and a store",
            "past the end of the memory would trap instead of answering. Reaching",
            "here means there is no room for the return area at the bottom of a",
            "fresh heap, which cannot be reported through a return area that does",
            "not fit -- so it is the one failure with nowhere to go.",
        ],
    ));
    out.push_str(
        "    (if (i32.gt_u (global.get $mem_ptr)\n\
         \x20                 (i32.mul (memory.size) (i32.const 65536)))\n\
         \x20     (then unreachable))\n",
    );
}

/// A `u64` of 2^63 or more has no `Uint63.int`.
///
/// One check per `u64` a parameter can carry. No parameter is exempt and one
/// of them being in range says nothing about the others, so they are not
/// folded together -- and all of them run before `main_function`, so a
/// rejected call has not allocated anything.
///
/// A `u64` inside a sum is checked under the discriminants that make it live.
/// Not an optimisation: the canonical ABI zero-pads the flat slots a case does
/// not use, so checking an unused payload slot unconditionally would pass --
/// but reporting a *failure* read out of a slot that carries no value would be
/// an answer invented out of padding, and the guard is what rules that out.
fn range_checks(out: &mut String, param: &super::plan::Param, case: u32) {
    for (what, guards, slot) in int63s(param) {
        out.push('\n');
        out.push_str(&comment::wat(
            "    ",
            &[
                format!("{what} as a `u64` of 2^63 or more has no `Uint63.int`, so it"),
                "is rejected before the program runs rather than silently losing".into(),
                "its top bit. Read as an i64 that is exactly the negative half.".into(),
            ],
        ));
        let check = format!(
            "(if (i64.lt_s (local.get ${slot}) (i64.const 0))\n\
             \x20 (then\n\
             \x20   (call $err (local.get $ret) (i32.const {case}))\n\
             \x20   (return (local.get $ret))))",
        );
        out.push_str(&guarded(&guards, &check, "    "));
    }
}

/// Every `u64` a parameter can carry: how to describe it, the discriminants
/// that have to hold for it to be one, and the slot it arrives in.
fn int63s(param: &super::plan::Param) -> Vec<(String, Vec<String>, String)> {
    fn walk(
        repr: &Repr,
        slots: &[super::plan::Slot],
        what: String,
        guards: Vec<String>,
        found: &mut Vec<(String, Vec<String>, String)>,
    ) {
        match repr {
            Repr::Int63 => found.push((what, guards, slots[0].local.clone())),
            Repr::Bool | Repr::Enum { .. } => {}
            Repr::Sum(sum) => {
                for (discriminant, case) in sum.cases.iter().enumerate() {
                    let Some(payload) = &case.payload else {
                        continue;
                    };
                    let mut guards = guards.clone();
                    guards.push(format!(
                        "(i32.eq (local.get ${}) (i32.const {discriminant}))",
                        slots[0].local
                    ));
                    walk(
                        payload,
                        &slots[1..],
                        format!("{what}'s `{}` payload", case.wit_name),
                        guards,
                        found,
                    );
                }
            }
        }
    }
    let mut found = Vec::new();
    walk(
        &param.repr,
        &param.slots,
        format!("`{}`", param.wit_name),
        Vec::new(),
        &mut found,
    );
    found
}

/// A statement under the conditions that have to hold for it to run, one
/// `if` per condition so that the nesting reads as the case analysis it is.
///
/// The closing parens fold onto the last line, which is how the hand-written
/// adapters close a nested `if` -- and `checks/generated.py` compares tokens,
/// so an extra line of its own would be a difference in nothing.
fn guarded(guards: &[String], statement: &str, indent: &str) -> String {
    let mut out = String::new();
    for (depth, guard) in guards.iter().enumerate() {
        let pad = "  ".repeat(depth * 2);
        writeln!(out, "{indent}{pad}(if {guard}").unwrap();
        writeln!(out, "{indent}{pad}  (then").unwrap();
    }
    let pad = "  ".repeat(guards.len() * 2);
    let lines: Vec<&str> = statement.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        write!(out, "{indent}{pad}{line}").unwrap();
        match i + 1 == lines.len() {
            // `(then` and `(if`, innermost first.
            true => writeln!(out, "{}", ")".repeat(2 * guards.len())).unwrap(),
            false => out.push('\n'),
        }
    }
    out
}

/// Hold the incoming value to what its WIT type promises.
///
/// A discriminant out of range names no constructor. The canonical ABI says a
/// conforming caller cannot send one, and trapping is what the component model
/// allows for a caller that does anyway -- strictly better than building a
/// value the Rocq side has no case for and matching on it.
///
/// Nested discriminants are checked under the ones that select them, for the
/// reason `range_checks` gives: the slots of a case that was not sent are
/// padding, and a conclusion drawn from padding is invented.
fn validate_param(out: &mut String, param: &super::plan::Param) {
    for (lines, guards, statement) in bounds(param) {
        out.push('\n');
        out.push_str(&comment::wat("    ", &lines));
        out.push_str(&guarded(&guards, &statement, "    "));
    }
}

/// Every discriminant a parameter carries that could be out of range: what to
/// say about it, the discriminants that select it, and the trap.
fn bounds(param: &super::plan::Param) -> Vec<(Vec<String>, Vec<String>, String)> {
    fn trap(slot: &str, cases: usize) -> String {
        format!(
            "(if (i32.ge_u (local.get ${slot}) (i32.const {cases}))\n\
             \x20 (then unreachable))",
        )
    }
    fn walk(
        repr: &Repr,
        slots: &[super::plan::Slot],
        what: String,
        guards: Vec<String>,
        found: &mut Vec<(Vec<String>, Vec<String>, String)>,
    ) {
        match repr {
            // `bool` is normalised rather than trapped on, in `to_rocq`: the
            // canonical ABI lifts any non-zero byte as `true`, so clamping is
            // what it asks for.
            Repr::Bool | Repr::Int63 => {}
            Repr::Enum {
                cases, rocq_type, ..
            } => found.push((
                vec![
                    format!("`{rocq_type}` has {cases} constructors, so {what} being"),
                    format!("{cases} or more names none of them."),
                ],
                guards,
                trap(&slots[0].local, *cases),
            )),
            Repr::Sum(sum) => {
                found.push((
                    vec![
                        format!("`{}` has two cases, so {what} being 2 or", sum.wit),
                        "more names neither of them.".into(),
                    ],
                    guards.clone(),
                    trap(&slots[0].local, sum.cases.len()),
                ));
                for (discriminant, case) in sum.cases.iter().enumerate() {
                    let Some(payload) = &case.payload else {
                        continue;
                    };
                    let mut guards = guards.clone();
                    guards.push(format!(
                        "(i32.eq (local.get ${}) (i32.const {discriminant}))",
                        slots[0].local
                    ));
                    walk(
                        payload,
                        &slots[1..],
                        format!("the discriminant of its `{}` payload", case.wit_name),
                        guards,
                        found,
                    );
                }
            }
        }
    }
    let mut found = Vec::new();
    walk(
        &param.repr,
        &param.slots,
        format!("`{}`", param.wit_name),
        Vec::new(),
        &mut found,
    );
    found
}

/// Run the program, apply the arguments one at a time, and check the allocator
/// after every step that can have given up.
///
/// The loop is the whole of the several-parameter case. A compiled Rocq
/// function takes one argument per call, so applying argument *i* leaves the
/// closure for argument *i+1* in `result`, read exactly as the one
/// `main_function` leaves there. There is no 2-ary core function to call:
/// every function in the table of a compiled `int -> int -> int` has the
/// `(env, arg)` type, which `examples/minus` measured (TODO.md#arity).
fn call(out: &mut String, e: &Export) {
    let d = &e.discriminants;
    let oom = |out: &mut String, lines: &[String]| {
        out.push_str(&comment::wat("    ", lines));
        write!(
            out,
            "    (if (global.get $out_of_mem)\n\
             \x20     (then\n\
             \x20       (call $err (local.get $ret) (i32.const {}))\n\
             \x20       (return (local.get $ret))))\n",
            d.out_of_memory
        )
        .unwrap()
    };

    out.push('\n');
    out.push_str(&comment::wat(
        "    ",
        &[
            format!(
                "`{}` runs the whole program and leaves the export",
                exports::MAIN_FUNCTION
            ),
            format!(
                "closure in `{}`. It allocates, so it can be what",
                exports::RESULT
            ),
            "exhausts the memory.".into(),
        ],
    ));
    out.push_str("    (call $main_function)\n");
    oom(
        out,
        &[
            "`out_of_mem` means the value in `result` was never written, so".to_string(),
            "this check is not optional.".into(),
        ],
    );
    out.push_str("    (local.set $closure (global.get $result))\n");

    let n = e.params.len();
    for (i, p) in e.params.iter().enumerate() {
        let last = i + 1 == n;

        // Canonical ABI -> CertiRocq, the direction an export goes. The
        // prologue is the store into the cell, for a representation that has
        // one; the value is what the application is handed.
        let base = match &p.block {
            Some(block) => format!("(local.get ${})", block.local),
            None => String::new(),
        };
        let lowered = convert::to_rocq(
            &p.repr,
            &convert::Flat::locals(&p.slots),
            match &p.block {
                Some(block) => &block.cells,
                None => &Cells::Nothing,
            },
            convert::Base(&base),
        );
        let mut lines = vec![];
        if n > 1 {
            lines.push(format!("Argument {} of {n}, `{}`.", i + 1, p.wit_name));
        }
        match &p.repr {
            // A sum is built inline rather than in a prologue: the
            // discriminant picks the constructor, so the `if` *is* the value.
            Repr::Sum(sum) => {
                lines.extend([
                    format!("`{}` is built here. The discriminant picks the", sum.wit),
                    "constructor, so only the case that was sent is written:".into(),
                ]);
                lines.extend(sum.cases.iter().enumerate().map(|(d, case)| {
                    match (&case.payload, case.tag) {
                        (_, super::plan::CtorTag::Immediate(immediate)) => {
                            format!("{d} -> `{}`, the immediate {immediate}.", case.ctor)
                        }
                        (_, super::plan::CtorTag::Boxed(ordinal)) => {
                            format!("{d} -> `{}`, a pointer to [{ordinal}, payload].", case.ctor)
                        }
                    }
                }));
                out.push('\n');
                out.push_str(&comment::wat("    ", &lines));
            }
            _ if !lowered.prologue.is_empty() => {
                lines.extend([
                    format!(
                        "A primitive int is boxed: a pointer to its {}-byte payload,",
                        repr::PRIM_PAYLOAD_SIZE
                    ),
                    "which is what gets passed, not the value. repr.rs".into(),
                    "PRIM_PAYLOAD_SIZE.".into(),
                ]);
                out.push('\n');
                out.push_str(&comment::wat("    ", &lines));
                for line in &lowered.prologue {
                    writeln!(out, "    {line}").unwrap();
                }
            }
            _ => {}
        }

        out.push('\n');
        let mut lines = vec![];
        if i == 0 {
            lines.extend([
                format!(
                    "Closure layout: [tag, func_idx, env_ptr] in {}-byte cells, so the",
                    repr::CELL
                ),
                format!(
                    "environment is at offset {} and the index at offset {}.",
                    repr::CLOSURE_ENV_OFFSET,
                    repr::CLOSURE_FUNC_IDX_OFFSET
                ),
                "repr.rs CLOSURE_ENV_OFFSET, CLOSURE_FUNC_IDX_OFFSET.".into(),
            ]);
        } else {
            lines.extend([
                format!(
                    "Application {} of {n}, through the closure the one before",
                    i + 1
                ),
                format!(
                    "it left in `{}`. The same code as the first: a",
                    exports::RESULT
                ),
                "partial application is an ordinary closure.".into(),
            ]);
        }
        out.push_str(&comment::wat("    ", &lines));
        write!(
            out,
            "    (call_indirect (type $closure)\n\
             \x20     (i32.load offset={env} (local.get $closure))\n\
             \x20     {lowered}\n\
             \x20     (i32.load offset={func} (local.get $closure)))\n\n",
            env = repr::CLOSURE_ENV_OFFSET,
            func = repr::CLOSURE_FUNC_IDX_OFFSET,
            lowered = lowered.value.replace('\n', "\n      "),
        )
        .unwrap();

        oom(
            out,
            &match last {
                true => vec![
                    "The call allocated its own result, so this is the last place".to_string(),
                    "the flag can have been set.".into(),
                ],
                // Not "or the failure goes unreported": `out_of_mem` is only
                // ever set, so the last check would see this too. What this
                // one prevents is the call below it.
                false => vec![
                    "Applying one argument of several allocates -- an environment".to_string(),
                    format!(
                        "and the next closure -- so the flag can be set here with `{}`",
                        exports::RESULT
                    ),
                    "still holding the closure from before. Checking now is what".into(),
                    "stops the next application calling through that stale".into(),
                    "closure, which expects an argument of the shape this one".into(),
                    "took: a wrong answer here, a trap once a body matches on an".into(),
                    "ordinal (TODO.md#structured-types).".into(),
                ],
            },
        );

        if !last {
            out.push_str("    (local.set $closure (global.get $result))\n");
        }
    }
}

/// Write the answer into the return area.
fn lift(out: &mut String, e: &Export) {
    out.push('\n');
    out.push_str(&comment::wat(
        "    ",
        &match &e.ok {
            Repr::Int63 => vec![
                format!(
                    "`{}` holds a pointer to the {}-byte payload, not",
                    exports::RESULT,
                    repr::PRIM_PAYLOAD_SIZE
                ),
                "the value itself.".into(),
            ],
            Repr::Bool | Repr::Enum { .. } => vec![
                format!(
                    "`{}` holds the immediate itself: nothing was",
                    exports::RESULT
                ),
                "allocated to return a nullary constructor, and `2*ord+1`".into(),
                "inverts to a shift.".into(),
            ],
            Repr::Sum(sum) => vec![
                format!(
                    "`{}` holds the `{}` itself -- a pointer for the case",
                    exports::RESULT,
                    sum.rocq_type
                ),
                "that carries a payload, an immediate for the one that does".into(),
                "not -- and `$ok` writes whichever it is into the return area.".into(),
            ],
        },
    ));
    // CertiRocq -> canonical ABI, the direction an export's result goes --
    // and the one an import wrapper's *arguments* would go, which is why it
    // is emitted from `convert` rather than from here.
    //
    // A sum has no single core value to lift into, so `$ok` is handed the Rocq
    // value and does the writing (`convert::store_canonical`).
    let lifted = match &e.ok {
        Repr::Sum(_) => convert::Conversion {
            prologue: Vec::new(),
            value: format!("(global.get ${})", exports::RESULT),
        },
        ok => convert::from_rocq(ok, &format!("(global.get ${})", exports::RESULT)),
    };
    for line in &lifted.prologue {
        writeln!(out, "    {line}").unwrap();
    }
    writeln!(out, "    (call $ok (local.get $ret) {})", lifted.value).unwrap();
}

fn result_type(e: &Export) -> String {
    e.signature
        .rsplit_once("-> ")
        .map(|(_, r)| r.to_string())
        .unwrap_or_else(|| "the result type".into())
}

fn error_cases(d: &Discriminants) -> String {
    let mut cases = vec![format!("out-of-memory = {}", d.out_of_memory)];
    if let Some(case) = d.out_of_range {
        cases.push(format!("out-of-range = {case}"));
    }
    cases.join(", ")
}

fn int_bytes(int: Int) -> u32 {
    match int {
        Int::U8 => 1,
        Int::U16 => 2,
        Int::U32 => 4,
        Int::U64 => 8,
    }
}
