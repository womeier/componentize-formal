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

/// `result<u64, error>` is 16 bytes at align 8, discriminant at 0 and payload
/// at 8 -- and the adapter reserves the 16, rounded up, plus 8 for the boxed
/// argument. These are the numbers the hand-written plus1 adapter contains.
#[test]
fn a_u64_result_is_sixteen_bytes_at_align_eight() {
    let plan = plan_for("plus1", PLUS1);
    let l = &plan.export.layout;
    assert_eq!((l.ret_size, l.ret_align, l.payload), (16, 8, 8));
    assert_eq!(l.reserve, 24, "16 for the return area, 8 for the boxed int");
    assert_eq!(l.arg, Some(16), "the argument sits above the return area");
    assert_eq!(plan.export.param.wasm, "i64");
    assert_eq!(plan.export.param.repr, Repr::Int63);
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
    assert_eq!(l.arg, None, "an immediate needs no heap");
    assert_eq!(plan.export.param.wasm, "i32");
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

    let Repr::Enum { cases, .. } = &plan.export.param.repr else {
        panic!("a colour parameter is an enum");
    };
    assert_eq!(*cases, 3);
    assert!(
        plan.export.param.repr.tags_are_the_formula(),
        "the generator wrote the inductive, so ordinal == discriminant"
    );
    assert_eq!(plan.export.param.repr.tags(), [1, 3, 5]);
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
