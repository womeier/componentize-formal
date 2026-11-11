//! What the generator emits, held to the numbers it cannot be allowed to get
//! wrong.
//!
//! The end-to-end evidence lives in `checks/generated.py`, which generates for
//! the worlds under `examples/`, compares the output against the hand-written
//! adapters token for token and then runs the components. These tests cover
//! what that cannot: the layout arithmetic for each shape, and the refusals.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use componentize_formal::config::Config;
use componentize_formal::emit::{self, plan::Repr, Plan};
use componentize_formal::resolve_world;

/// A world written to a scratch directory, with the error package vendored
/// beside it so the layout matches what `component embed` will read.
///
/// A fresh directory per call, not per world: the tests run as threads of one
/// process, so two of them setting up the same fixture raced on these files.
/// One read a half-written `errors.wit`, which resolution reported as a tree
/// that does not vendor the package -- and that answer, in the resolver this
/// replaced, was a panic.
fn world(name: &str, wit: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "bindgen-generate-{}-{name}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let deps = dir.join("wit/deps/certirocq-abi");
    fs::create_dir_all(&deps).unwrap();
    fs::write(deps.join("errors.wit"), componentize_formal::ABI_WIT).unwrap();
    fs::write(dir.join("wit/world.wit"), wit).unwrap();
    dir
}

fn plan_for(name: &str, wit: &str) -> Plan {
    let dir = world(name, wit);
    let cfg = Config::for_wit(dir.join("wit"));
    let (resolve, id) = resolve_world(&cfg.wit, None).expect("the fixture resolves");
    Plan::new(&resolve, id, &cfg, "wit").expect("the fixture plans")
}

fn refusal(name: &str, wit: &str) -> String {
    let dir = world(name, wit);
    let cfg = Config::for_wit(dir.join("wit"));
    let (resolve, id) = resolve_world(&cfg.wit, None).expect("the fixture resolves");
    format!(
        "{:#}",
        Plan::new(&resolve, id, &cfg, "wit").expect_err("must be refused")
    )
}

const PLUS1: &str = r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export plus1: func(val: u64) -> result<u64, error>;
}
"#;

const NEGB: &str = r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export negb: func(b: bool) -> result<bool, error>;
}
"#;

const COLOUR: &str = r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  enum colour { red, green, blue }
  export next: func(c: colour) -> result<colour, error>;
}
"#;

const MINUS: &str = r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export minus: func(a: u64, b: u64) -> result<u64, error>;
}
"#;

/// A `u64` on the way out and none on the way in: the direction that cannot
/// narrow, so nothing about it needs checking.
const WIDEN: &str = r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export widen: func(b: bool) -> result<u64, error>;
}
"#;

/// Mixed boxing, to pin that the boxes are numbered over the *boxed*
/// parameters rather than over all of them.
const MIXED: &str = r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export pick: func(which: bool, val: u64) -> result<u64, error>;
}
"#;

/// `result<u64, error>` is 16 bytes at align 8, discriminant at 0 and payload
/// at 8 -- and the adapter reserves the 16, rounded up, plus 8 for the boxed
/// argument. These are the numbers the hand-written plus1 adapter contains.
#[test]
fn a_u64_result_is_sixteen_bytes_at_align_eight() {
    let plan = plan_for("plus1", PLUS1);
    let l = &plan.export.layout;
    assert_eq!((l.ret_size, l.ret_align, l.payload), (16, 8, 8));
    assert_eq!(l.reserve, 24, "16 for the return area, 8 for the boxed int");
    let p = &plan.export.params[0];
    assert_eq!(
        p.box_offset,
        Some(16),
        "the argument sits above the return area"
    );
    assert_eq!(p.wasm, "i64");
    assert_eq!(p.repr, Repr::Int63);
    assert_eq!(
        p.box_local.as_deref(),
        Some("arg"),
        "one parameter, so the box local is the `$arg` the hand-written plus1 \
         adapter uses"
    );
}

/// `result<bool, error>` is 2 bytes at align 1, so the payload is at offset 1
/// rather than 8: the same `result` former, a different layout, and neither
/// number is a constant of the generator.
#[test]
fn a_bool_result_is_two_bytes_at_align_one() {
    let plan = plan_for("negb", NEGB);
    let l = &plan.export.layout;
    assert_eq!((l.ret_size, l.ret_align, l.payload), (2, 1, 1));
    assert_eq!(l.reserve, 8, "rounded up: mem_ptr has to stay 4-aligned");
    let p = &plan.export.params[0];
    assert_eq!(p.box_offset, None, "an immediate needs no heap");
    assert_eq!(p.box_local, None);
    assert_eq!(p.wasm, "i32");
}

/// Every boxed parameter gets its own box, all of them live at once: the first
/// application captures its box by address and the last application can still
/// be reading it, so they may not overlap or be reused. These are the offsets
/// the hand-written `examples/minus` adapter contains.
#[test]
fn each_boxed_parameter_gets_its_own_box() {
    let plan = plan_for("minus", MINUS);
    let l = &plan.export.layout;
    assert_eq!(
        l.reserve, 32,
        "16 for the return area, then 8 per boxed argument"
    );
    let offsets: Vec<_> = plan.export.params.iter().map(|p| p.box_offset).collect();
    assert_eq!(offsets, [Some(16), Some(24)]);
    let locals: Vec<_> = plan
        .export
        .params
        .iter()
        .map(|p| p.box_local.clone())
        .collect();
    assert_eq!(
        locals,
        [Some("arg_a".to_string()), Some("arg_b".to_string())],
        "with several parameters each box is named after its own"
    );
}

/// An immediate parameter takes no box, so the boxes are numbered over the
/// boxed parameters and not over the parameter list. Getting this wrong would
/// leave `val`'s box at the offset nothing reserved.
#[test]
fn an_immediate_parameter_takes_no_box() {
    let plan = plan_for("pick", MIXED);
    let l = &plan.export.layout;
    assert_eq!(l.reserve, 24, "16 for the return area, 8 for the one box");
    let offsets: Vec<_> = plan.export.params.iter().map(|p| p.box_offset).collect();
    assert_eq!(offsets, [None, Some(16)]);
    let locals: Vec<_> = plan
        .export
        .params
        .iter()
        .map(|p| p.box_local.clone())
        .collect();
    assert_eq!(locals, [None, Some("arg_val".to_string())]);
}

/// The Module Type is curried, one arrow per parameter, because that is what
/// the compiled term is: the adapter applies the arguments one at a time. A
/// tuple would type-check against an implementation the adapter cannot call.
#[test]
fn several_parameters_are_curried_in_the_interface() {
    let plan = plan_for("minus", MINUS);
    let interface = emit::rocq::interface(&plan);
    assert!(
        interface.contains("Parameter minus : int -> int -> int."),
        "{interface}"
    );
}

/// Out of range is only a failure for a type that has values the other side
/// cannot hold, so only then does the adapter get a discriminant for it. The
/// converse matters more: a range check with nothing to report through would
/// store a case the world never declared.
#[test]
fn only_a_narrowing_parameter_gets_a_range_check() {
    let plus1 = plan_for("plus1", PLUS1);
    assert_eq!(plus1.export.discriminants.out_of_memory, 0);
    assert_eq!(plus1.export.discriminants.out_of_range, Some(1));
    assert_eq!(plus1.export.failures.len(), 2);

    let negb = plan_for("negb", NEGB);
    assert_eq!(negb.export.discriminants.out_of_range, None);
    assert_eq!(negb.export.failures.len(), 1, "only out of memory");
}

/// Where the boundary is, in the one place it is actually enforced.
///
/// `Uint63.int` holds 0..2^63-1, a `u64` holds 0..2^64-1, and the whole top
/// half has no counterpart. The check is a *signed* comparison against zero
/// because an i64 is negative on exactly those values -- so this pins both the
/// boundary and the one-sided-ness of it: `i64.lt_s ... 0` rejects 2^63 and
/// 2^64-1 alike, where an equality against 2^63, or an unsigned comparison,
/// would let most of the top half through to lose its high bit in silence.
/// `checks/backend.py` runs both ends of that half against the real backend.
#[test]
fn the_range_check_is_the_2_63_boundary() {
    let adapter = emit::adapter::adapter(&plan_for("plus1", PLUS1));
    assert!(
        adapter.contains("(if (i64.lt_s (local.get $val) (i64.const 0))"),
        "{adapter}"
    );

    // And before the program runs, not after: a rejected call must not have
    // allocated, and `main_function` is what allocates.
    let check = adapter
        .find("(i64.lt_s (local.get $val)")
        .expect("the range check");
    let run = adapter
        .find("(call $main_function)")
        .expect("the call into the backend");
    assert!(
        check < run,
        "the range check runs after the program:\n{adapter}"
    );

    // The u64 is stored into its box only once past the check, for the same
    // reason.
    let store = adapter
        .find("(i64.store (local.get $arg")
        .expect("the argument store");
    assert!(check < store, "{adapter}");
}

/// The other direction never narrows: a 63-bit int lifts into a `u64`
/// exactly, so a world with a `u64` result and no `u64` parameter gets no
/// range check and no discriminant to report one through.
///
/// Worth asserting rather than assuming, because the cost of being wrong is
/// asymmetric. A missing check loses a value's top bit quietly; a check that
/// should not be there stores a case the world never declared, which is
/// `plan.rs`'s converse assertion -- and neither shows up as a build failure.
#[test]
fn a_u64_result_needs_no_range_check() {
    let plan = plan_for("widen", WIDEN);
    assert_eq!(plan.export.discriminants.out_of_range, None);
    assert_eq!(plan.export.failures.len(), 1, "only out of memory");
    assert_eq!(plan.export.ok, Repr::Int63, "the result is still carried");

    let adapter = emit::adapter::adapter(&plan);
    assert_eq!(
        adapter.matches("i64.lt_s").count(),
        0,
        "nothing to range-check:\n{adapter}"
    );
    // The lift itself is there, so this is not passing because the result went
    // missing.
    assert!(adapter.contains("(i64.load"), "{adapter}");
}

/// The emitters do not trust the gate. An `option` that reached them anyway --
/// the gate and the plan are separate code and can drift -- is refused with a
/// message that says which layer was supposed to catch it, rather than
/// emitting an adapter for a type with no representation.
#[test]
fn an_option_is_refused_by_the_emitter_too() {
    let err = refusal(
        "option",
        r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export unwrap: func(x: option<u64>) -> result<u64, error>;
}
"#,
    );
    assert!(err.contains("option"), "{err}");
    assert!(err.contains("no Rocq counterpart"), "{err}");
    assert!(err.contains("the gate should have rejected"), "{err}");

    // Same for the result position, which is a different call into `repr_of`.
    let err = refusal(
        "nested-option",
        r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export lookup: func(k: u64) -> result<option<u64>, error>;
}
"#,
    );
    assert!(err.contains("no Rocq counterpart"), "{err}");
    assert!(err.contains("the result of `lookup`"), "{err}");
}

/// Every parameter that can be out of range gets its own check, and there is
/// one application per parameter. The count is what matters: a loop that
/// emitted the check or the call once would still produce a module that
/// assembles and answers -- wrongly, for the arguments it skipped.
#[test]
fn every_parameter_is_checked_and_applied() {
    let adapter = emit::adapter::adapter(&plan_for("minus", MINUS));
    assert_eq!(
        adapter.matches("(i64.lt_s (local.get").count(),
        2,
        "one range check per u64 parameter:\n{adapter}"
    );
    assert_eq!(
        adapter.matches("(call_indirect (type $closure)").count(),
        2,
        "one application per parameter:\n{adapter}"
    );
    assert_eq!(
        adapter.matches("(if (global.get $out_of_mem)").count(),
        3,
        "main_function, then one per application:\n{adapter}"
    );
    // The partial application is read back out of `result`, once per
    // application but not after the last: that one holds the answer.
    assert_eq!(
        adapter
            .matches("(local.set $closure (global.get $result))")
            .count(),
        2,
        "after main_function, and between the two applications:\n{adapter}"
    );
}

/// An enum becomes an inductive whose constructors are in the world's case
/// order, because that order is the discriminant on one side and the ordinal
/// on the other.
#[test]
fn an_enum_becomes_an_inductive_in_declaration_order() {
    let plan = plan_for("colour", COLOUR);
    let ind = &plan.inductives[0];
    assert_eq!(ind.rocq_name, "colour");
    assert_eq!(ind.constructors, ["red", "green", "blue"]);
    assert_eq!(ind.wit_declaration(), "enum colour { red, green, blue }");

    let Repr::Enum { cases, .. } = &plan.export.params[0].repr else {
        panic!("a colour parameter is an enum");
    };
    assert_eq!(*cases, 3);
    assert!(
        plan.export.params[0].repr.tags_are_the_formula(),
        "the generator wrote the inductive, so ordinal == discriminant"
    );
    assert_eq!(plan.export.params[0].repr.tags(), [1, 3, 5]);
}

/// `bool`'s immediates are the formula too, but for the other reason: the
/// pipeline's swap lines its ordinals up with `false = 0, true = 1`. If that
/// ever stops holding, the shift the adapter lifts and lowers with stops being
/// valid, and this is what notices.
#[test]
fn bool_tags_are_the_formula_because_of_the_swap() {
    assert_eq!(Repr::Bool.tags(), [1, 3]);
    assert!(Repr::Bool.tags_are_the_formula());
}

/// The world is where the Rocq names come from, so a name that is not a Rocq
/// identifier has to be refused rather than mangled into one the user did not
/// write.
#[test]
fn names_that_are_not_rocq_identifiers_are_refused() {
    let err = refusal(
        "keyword",
        r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export %let: func(val: u64) -> result<u64, error>;
}
"#,
    );
    assert!(err.contains("`let`"), "{err}");
    assert!(err.contains("rename"), "{err}");
}

/// The parameter's name is also a wat local, so it has to clear the locals the
/// adapter declares for itself. Two `$ret`s in one function is not a subtle
/// failure, but it is one the generator must never emit.
#[test]
fn a_parameter_cannot_shadow_an_adapter_local() {
    let err = refusal(
        "local",
        r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export plus1: func(ret: u64) -> result<u64, error>;
}
"#,
    );
    assert!(err.contains("already taken"), "{err}");
}

/// A `result<_, error>` passes the gate -- it has an error channel -- but
/// leaves the Rocq function with nothing to return.
#[test]
fn a_result_with_no_ok_type_is_refused() {
    let err = refusal(
        "no-ok",
        r#"package local:demo;
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export go: func(val: u64) -> result<_, error>;
}
"#,
    );
    assert!(
        err.contains("nothing for the Rocq function to return"),
        "{err}"
    );
}

/// Every generated file says so, and the writer looks for exactly that line
/// before overwriting one.
#[test]
fn every_generated_file_carries_the_marker() {
    for (name, wit) in [("plus1", PLUS1), ("negb", NEGB), ("colour", COLOUR)] {
        let plan = plan_for(name, wit);
        for artifact in emit::artifacts(&plan) {
            assert!(
                artifact.contents.contains(emit::MARKER),
                "{name}: {} has no marker",
                artifact.path.display()
            );
        }
    }
}

/// Four files, and the names the build rules tie together.
#[test]
fn the_artifacts_are_the_four_files() {
    let plan = plan_for("plus1", PLUS1);
    let paths: Vec<String> = emit::artifacts(&plan)
        .iter()
        .map(|a| a.path.display().to_string())
        .collect();
    assert_eq!(
        paths,
        ["interface.v", "extraction.v", "adapter.wat", "Makefile"]
    );
}

/// A file with no marker may be someone's own work, so the run stops before
/// writing anything at all -- not after the first few files.
#[test]
fn hand_written_files_are_not_overwritten() {
    let plan = plan_for("plus1", PLUS1);
    let dir = std::env::temp_dir().join(format!("bindgen-clobber-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let mine = dir.join("interface.v");
    fs::write(&mine, "(* mine, actually *)\n").unwrap();

    let artifacts = emit::artifacts(&plan);
    let err = format!(
        "{:#}",
        emit::write(&dir, &artifacts, false).expect_err("must refuse")
    );
    assert!(err.contains("may be yours"), "{err}");
    assert_eq!(
        fs::read_to_string(&mine).unwrap(),
        "(* mine, actually *)\n",
        "the file was rewritten anyway"
    );
    assert!(
        !dir.join("adapter.wat").exists(),
        "nothing else should have been written either"
    );

    emit::write(&dir, &artifacts, true).expect("--force overwrites");
    assert!(fs::read_to_string(&mine).unwrap().contains(emit::MARKER));

    // Second time round, the files it wrote are its own to rewrite.
    emit::write(&dir, &artifacts, false).expect("a generated file is rewritten");
    fs::remove_dir_all(&dir).ok();
}

/// The vendored package has to be where `component embed` looks, and has to be
/// left alone once it is there: the build reads that copy, so a local change
/// to it may be deliberate.
#[test]
fn the_error_package_is_vendored_beside_the_world() {
    let plan = plan_for("plus1", PLUS1);
    let artifact = emit::vendored_abi(&plan);
    assert_eq!(
        artifact.path,
        Path::new("wit/deps/certirocq-abi/errors.wit"),
        "the directory name is the package name"
    );
    assert_eq!(artifact.overwrite, emit::Overwrite::IfMissing);
    assert_eq!(artifact.contents, componentize_formal::ABI_WIT);
}

/// The round-trip harness is for `checks/roundtrip.py` and nothing else. If it
/// ever appeared in `artifacts`, `generate` would write a module into the
/// user's project that their component does not need and their Makefile would
/// not know what to do with.
#[test]
fn the_roundtrip_harness_is_not_part_of_a_build() {
    let plan = plan_for("plus1", PLUS1);
    let (module, _) = emit::roundtrip::module(&plan);
    assert!(!module.is_empty());
    assert!(
        emit::artifacts(&plan)
            .iter()
            .all(|a| !a.contents.contains("canonical_int")),
        "the harness leaked into the files a component build writes"
    );
}

/// One pair per *representation*, not per position: `next` takes a `colour`
/// and returns one, and round-tripping the same conversion twice says nothing
/// the first run did not.
#[test]
fn a_representation_is_round_tripped_once() {
    let (_, pairs) = emit::roundtrip::module(&plan_for("colour", COLOUR));
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].rocq_type, "colour");

    // Mixed boxing is two representations in one world, and both are run.
    let (_, pairs) = emit::roundtrip::module(&plan_for("mixed", MIXED));
    let types: Vec<&str> = pairs.iter().map(|p| p.rocq_type.as_str()).collect();
    assert_eq!(types, ["bool", "int"]);
}

/// Both orders, for every pair: the harness is the only place the Rocq-side
/// direction runs, which is the half an import wrapper will be built on.
#[test]
fn every_pair_is_exported_in_both_orders() {
    let (module, pairs) = emit::roundtrip::module(&plan_for("mixed", MIXED));
    for pair in &pairs {
        for name in [&pair.canonical, &pair.rocq] {
            assert!(
                module.contains(&format!("(export \"{name}\")")),
                "{name} is in the manifest but not in the module:\n{module}"
            );
        }
        assert!(!pair.canonical_values.is_empty());
        assert_eq!(pair.canonical_values.len(), pair.rocq_values.len());
    }
}

/// The values an immediate is run with are `2*ord+1` of the canonical ones,
/// and the ends of `Uint63.int`'s range are among a boxed int's. The check
/// restates both; this is the same statement where a failure is cheap to read.
#[test]
fn the_values_are_the_ones_a_conversion_goes_wrong_on() {
    let (_, pairs) = emit::roundtrip::module(&plan_for("colour", COLOUR));
    assert_eq!(pairs[0].canonical_values, ["0", "1", "2"]);
    assert_eq!(pairs[0].rocq_values, ["1", "3", "5"]);

    let (_, pairs) = emit::roundtrip::module(&plan_for("plus1", PLUS1));
    let values = &pairs[0].canonical_values;
    assert!(values.contains(&"0".to_string()));
    assert!(values.contains(&"9223372036854775807".to_string()));
    assert!(
        values.iter().all(|v| v.parse::<u64>().unwrap() < 1 << 63),
        "2**63 has no Uint63.int, so the conversion is not what runs on it"
    );
    // The payload is the same number on both sides.
    assert_eq!(&pairs[0].rocq_values, values);
}

/// The manifest is what the check reads, so it has to parse as TOML and name
/// the exports the module has.
#[test]
fn the_manifest_names_what_the_module_exports() {
    let plan = plan_for("mixed", MIXED);
    let (module, pairs) = emit::roundtrip::module(&plan);
    let doc: toml::Value = toml::from_str(&emit::roundtrip::manifest(&plan, &pairs))
        .expect("the manifest parses as TOML");
    let listed = doc["pair"].as_array().expect("an array of pairs");
    assert_eq!(listed.len(), pairs.len());
    for pair in listed {
        let name = pair["canonical"].as_str().unwrap();
        assert!(module.contains(name), "{name} is not in the module");
    }
    assert_eq!(doc["world"].as_str(), Some("wrapper-world"));
}
