//! `adapter.wat`: the canonical-ABI glue.
//!
//! Every number in the emitted module comes from one of two places --
//! [`crate::repr`] for CertiRocq's side, `wit_parser::SizeAlign` for the
//! component model's -- and never from a literal here. The two hand-written
//! adapters under `examples/` were written before this emitter existed and are
//! now what it is held to, byte for byte, by `checks/generated.py`.

use std::fmt::Write;

use wit_parser::Int;

use super::plan::{Discriminants, Export, Plan, Repr, ADAPTER_ALIGN};
use super::{comment, BACKEND_MODULE, MARKER};
use crate::repr;
use crate::repr::exports;

/// Locals the emitted function declares for itself. A parameter named one of
/// these would be a duplicate name in the wat, so [`super::plan`] refuses it.
pub const LOCALS: &[&str] = &["ret", "arg", "closure"];

/// Where the argument is stored when it has to be boxed.
const ARG: &str = "$arg";

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
    writeln!(
        out,
        "  (func $ok (param $ret i32) (param $val {ty})\n\
         \x20   ({store_tag} (local.get $ret) (i32.const {ok}))\n\
         \x20   ({store} offset={payload} (local.get $ret) (local.get $val)))\n",
        ty = payload_wasm_type(&e.ok),
        store_tag = store(l.ret_tag),
        ok = d.ok,
        store = payload_store(&e.ok),
        payload = l.payload,
    )
    .unwrap();

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
        store_tag = store(l.ret_tag),
        err = d.err,
        store_case = store(l.err_tag),
        payload = l.payload,
    )
    .unwrap();

    // ---- the export --------------------------------------------------------
    writeln!(
        out,
        "\n  (func (export \"{}\") (param ${} {}) (result i32)",
        e.wit_name, e.param.rocq_name, e.param.wasm
    )
    .unwrap();
    out.push_str("    (local $ret i32)\n");
    if l.arg.is_some() {
        writeln!(out, "    (local {ARG} i32)").unwrap();
    }
    out.push_str("    (local $closure i32)\n");

    reserve(&mut out, e);
    if let Some(case) = d.out_of_range {
        range_check(&mut out, e, case);
    }
    validate_param(&mut out, e);
    call(&mut out, e);
    lift(&mut out, e);

    out.push_str("    (local.get $ret))\n)\n");
    out
}

/// Take the return area, and the boxed argument if there is one, off the
/// backend's own bump pointer.
fn reserve(out: &mut String, e: &Export) {
    let l = &e.layout;
    out.push('\n');
    out.push_str(&comment::wat(
        "    ",
        &[
            match l.arg {
                Some(_) => "The return area and the boxed argument come off the same bump",
                None => "The return area comes off the same bump",
            }
            .to_string(),
            format!(
                "pointer the backend allocates from, before `{}` runs,",
                exports::MAIN_FUNCTION
            ),
            "so the heap is still empty and the reservation cannot fail in".into(),
            "practice.".into(),
            String::new(),
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
        ],
    ));
    writeln!(
        out,
        "    (global.set $mem_ptr\n\
         \x20     (i32.and (i32.add (global.get $mem_ptr) (i32.const {})) (i32.const {})))",
        ADAPTER_ALIGN - 1,
        -(ADAPTER_ALIGN as i32),
    )
    .unwrap();
    out.push_str("    (local.set $ret (global.get $mem_ptr))\n");
    if let Some(arg) = l.arg {
        writeln!(
            out,
            "    (local.set {ARG} (i32.add (local.get $ret) (i32.const {arg})))"
        )
        .unwrap();
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
fn range_check(out: &mut String, e: &Export, case: u32) {
    let p = &e.param.rocq_name;
    out.push('\n');
    out.push_str(&comment::wat(
        "    ",
        &[
            "A `u64` of 2^63 or more has no `Uint63.int`, so it is rejected",
            "before the program runs rather than silently losing its top bit.",
            "Read as an i64 that is exactly the negative half.",
        ],
    ));
    write!(
        out,
        "    (if (i64.lt_s (local.get ${p}) (i64.const 0))\n\
         \x20     (then\n\
         \x20       (call $err (local.get $ret) (i32.const {case}))\n\
         \x20       (return (local.get $ret))))\n",
    )
    .unwrap();
}

/// Hold the incoming value to what its WIT type promises.
fn validate_param(out: &mut String, e: &Export) {
    let p = &e.param.rocq_name;
    match &e.param.repr {
        Repr::Enum {
            cases, rocq_type, ..
        } => {
            out.push('\n');
            out.push_str(&comment::wat(
                "    ",
                &[
                    format!(
                        "`{rocq_type}` has {cases} constructors, so a discriminant of {cases} or"
                    ),
                    "more names none of them. The canonical ABI says a conforming".into(),
                    "caller cannot send one, and trapping is what the component model".into(),
                    "allows for a caller that does anyway -- strictly better than".into(),
                    "building a value the Rocq side has no case for and matching on it.".into(),
                ],
            ));
            write!(
                out,
                "    (if (i32.ge_u (local.get ${p}) (i32.const {cases}))\n\
                 \x20     (then unreachable))\n",
            )
            .unwrap()
        }
        // `bool` is normalised rather than trapped on, in `lower` below: the
        // canonical ABI lifts any non-zero byte as `true`, so clamping is what
        // it asks for.
        Repr::Bool | Repr::Int63 => {}
    }
}

/// Run the program, get the closure, apply it, and check the allocator both
/// times it can have given up.
fn call(out: &mut String, e: &Export) {
    let d = &e.discriminants;
    let oom = |out: &mut String, lines: &[&str]| {
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
            "`out_of_mem` means the value in `result` was never written, so",
            "this check is not optional.",
        ],
    );
    out.push_str("    (local.set $closure (global.get $result))\n");

    if e.layout.arg.is_some() {
        out.push('\n');
        out.push_str(&comment::wat(
            "    ",
            &[
                format!(
                    "A primitive int is boxed: a pointer to its {}-byte payload,",
                    repr::PRIM_PAYLOAD_SIZE
                ),
                "which is what gets passed, not the value. repr.rs".into(),
                "PRIM_PAYLOAD_SIZE.".into(),
            ],
        ));
        writeln!(
            out,
            "    (i64.store (local.get {ARG}) (local.get ${}))",
            e.param.rocq_name
        )
        .unwrap();
    }

    out.push('\n');
    out.push_str(&comment::wat(
        "    ",
        &[
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
        ],
    ));
    write!(
        out,
        "    (call_indirect (type $closure)\n\
         \x20     (i32.load offset={env} (local.get $closure))\n\
         \x20     {lowered}\n\
         \x20     (i32.load offset={func} (local.get $closure)))\n\n",
        env = repr::CLOSURE_ENV_OFFSET,
        func = repr::CLOSURE_FUNC_IDX_OFFSET,
        lowered = lower(e),
    )
    .unwrap();
    oom(
        out,
        &[
            "The call allocated its own result, so this is the second place",
            "the flag can have been set.",
        ],
    );
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
        },
    ));
    writeln!(out, "    (call $ok (local.get $ret) {})", lift_expr(&e.ok)).unwrap();
}

/// The component's value, as the constructor the backend expects.
///
/// `(d << 1) | 1` is `2*ord+1` with the ordinal equal to the discriminant,
/// which holds for an `enum` because the generator writes the inductive in the
/// world's case order, and for `bool` because the pipeline's swap
/// (`CertiRocq.Plugin.Loader`: `bool => "bool" [ 1 0 ]`) happens to line its
/// ordinals up with the component model's `false = 0, true = 1`. Two reasons,
/// one shift -- and [`Repr::tags_are_the_formula`] is what keeps it honest if
/// either reason stops holding.
fn lower(e: &Export) -> String {
    let p = &e.param.rocq_name;
    match &e.param.repr {
        // The pointer to the payload stored just above, not the value.
        Repr::Int63 => format!("(local.get {ARG})"),
        // `i32.ne` first: the canonical ABI lifts any non-zero byte as `true`,
        // and a 2 shifted left would name a third constructor `bool` has not
        // got.
        Repr::Bool => format!(
            "(i32.or (i32.shl (i32.ne (local.get ${p}) (i32.const 0)) (i32.const 1)) (i32.const 1))"
        ),
        Repr::Enum { .. } => {
            format!("(i32.or (i32.shl (local.get ${p}) (i32.const 1)) (i32.const 1))")
        }
    }
}

/// The backend's value, as the component's.
fn lift_expr(ok: &Repr) -> String {
    match ok {
        Repr::Int63 => "(i64.load (global.get $result))".into(),
        Repr::Bool | Repr::Enum { .. } => "(i32.shr_u (global.get $result) (i32.const 1))".into(),
    }
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

/// The core type a payload of this representation is stored from.
fn payload_wasm_type(ok: &Repr) -> &'static str {
    match ok {
        Repr::Int63 => "i64",
        Repr::Bool | Repr::Enum { .. } => "i32",
    }
}

/// The store the ok payload needs, which is the payload's own width rather
/// than the return area's.
fn payload_store(ok: &Repr) -> String {
    match ok {
        Repr::Int63 => "i64.store".into(),
        // A `bool` is one byte in the canonical ABI; an enum's payload is its
        // discriminant, as wide as its case count makes it.
        Repr::Bool => store(Int::U8),
        Repr::Enum { tag, .. } => store(*tag),
    }
}

/// The store instruction for an integer of this width.
fn store(int: Int) -> String {
    match int {
        Int::U8 => "i32.store8".into(),
        Int::U16 => "i32.store16".into(),
        Int::U32 => "i32.store".into(),
        Int::U64 => "i64.store".into(),
    }
}

fn int_bytes(int: Int) -> u32 {
    match int {
        Int::U8 => 1,
        Int::U16 => 2,
        Int::U32 => 4,
        Int::U64 => 8,
    }
}
