//! Pin every fixture to the codes it should produce.
//!
//! Two structural guarantees matter more than the individual cases: nothing
//! under `wit/bad` may ever pass the gate, and nothing under `wit/good` may
//! ever fail it. Either direction would be a silent failure -- an adapter
//! generated for a world the backend cannot represent, or a usable world turned
//! away.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use componentize_formal::{resolve_world, support};

fn fixture(kind: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/wit")
        .join(kind)
        .join(name)
}

fn report(path: &Path) -> support::Report {
    let (resolve, world) =
        resolve_world(path, None).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
    support::check_world(&resolve, world)
}

fn codes(path: &Path) -> BTreeSet<String> {
    report(path)
        .rejections
        .iter()
        .map(|r| r.code.to_string())
        .collect()
}

fn expect_bad(name: &str, want: &[&str]) {
    let path = fixture("bad", name);
    let got = codes(&path);
    let want: BTreeSet<String> = want.iter().map(|s| s.to_string()).collect();
    assert_eq!(got, want, "bad/{name}");
}

fn wit_files(kind: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/wit")
        .join(kind);
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "wit"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures in {}", dir.display());
    paths
}

/// Nothing in `wit/good` may be turned away.
#[test]
fn good_worlds_pass() {
    for path in wit_files("good") {
        let report = report(&path);
        assert!(
            report.is_ok(),
            "{} was rejected:\n{}",
            path.display(),
            report
                .rejections
                .iter()
                .map(|r| r.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// Nothing in `wit/bad` may slip through.
#[test]
fn bad_worlds_are_rejected() {
    for path in wit_files("bad") {
        assert!(!report(&path).is_ok(), "{} passed the gate", path.display());
    }
}

#[test]
fn rejects_what_the_backend_cannot_represent() {
    // Two payload-less cases carry no value at all, and the Stdlib sum that
    // would hold them does not exist. Not "unbuilt": an `enum` says the same
    // thing and is carried today -- and it is refused at every depth, since
    // nesting it does not give it anything to carry.
    expect_bad("result-unit.wit", &["E-OPTION-UNIT"]);
    expect_bad("result-unit-nested.wit", &["E-OPTION-UNIT"]);
    expect_bad("list.wit", &["E-LIST"]);
    expect_bad("string.wit", &["E-STRING"]);
    expect_bad("char.wit", &["E-CHAR"]);
    expect_bad("flags.wit", &["E-FLAGS"]);
    // The resource type and its method are both reported.
    expect_bad("resource.wit", &["E-RESOURCE"]);
}

#[test]
fn rejects_what_is_merely_unbuilt() {
    expect_bad("signed.wit", &["E-SIGNED"]);
    expect_bad("float.wit", &["E-FLOAT"]);
    // The two shapes `E-STRUCTURED` still names. A `variant` for its
    // per-class ordinals; a `tuple` because nothing names it, so it would
    // have to ride on the Stdlib's `prod` -- and a `record` of the same
    // fields, which is carried, says the same thing.
    expect_bad("structured.wit", &["E-STRUCTURED"]);
    expect_bad("tuple.wit", &["E-STRUCTURED", "E-STRUCTURED"]);
    expect_bad("nullary.wit", &["E-ARITY-0"]);
    expect_bad("no-result.wit", &["E-NO-RESULT"]);
    expect_bad("multi-export.wit", &["E-MULTI-EXPORT"]);
    // u64 is the only integer width carried so far.
    for width in ["u8", "u16", "u32"] {
        expect_bad(&format!("int-{width}.wit"), &["E-INT-WIDTH"]);
    }
}

/// Nothing in the gate counts levels. Every answer it gives is a property of
/// the type and of the position -- parameter or result -- so there is no depth
/// at which it accepts what it would otherwise refuse, or the other way round.
/// The two halves of that:
#[test]
fn nesting_changes_no_answer() {
    // A sum carries a sum, to any depth the fixtures go.
    for name in ["option.wit", "nested-sum.wit", "nested-mixed.wit"] {
        let report = report(&fixture("good", name));
        assert!(
            report.is_ok(),
            "good/{name} was rejected:\n{}",
            report
                .rejections
                .iter()
                .map(|r| r.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    // And what is refused is refused inside one as well, naming the position
    // it sits in rather than the whole export.
    let nested: Vec<_> = report(&fixture("bad", "result-unit-nested.wit"))
        .rejections
        .into_iter()
        .map(|r| (r.code, r.site))
        .collect();
    assert_eq!(nested.len(), 1, "{nested:?}");
    assert_eq!(nested[0].0, "E-OPTION-UNIT");
    assert!(nested[0].1.contains("param `x`"), "{nested:?}");

    // A record is carried, so a refusal inside one has to come from the walk
    // over its fields rather than from the type around it -- and it names the
    // field, which is the only thing that points at the line to change.
    let field: Vec<_> = report(&fixture("bad", "record-tuple.wit"))
        .rejections
        .into_iter()
        .map(|r| (r.code, r.site))
        .collect();
    assert_eq!(field.len(), 1, "{field:?}");
    assert_eq!(field[0].0, "E-STRUCTURED");
    assert!(field[0].1.contains("param `c`, field `at`"), "{field:?}");
}

/// The one refusal that is neither a representation this backend lacks nor a
/// phase of the plan: a `record` with no fields. `wit-parser` accepts it and
/// the component model has no type for it, so `wasm-tools` refuses the
/// assembled component with "record type must have at least one
/// field" -- deep in the build, well after the line
/// that caused it. The gate says it while the field list is still on screen.
///
/// Nothing about the representation is in the way: a nullary constructor is an
/// immediate, which is what every `enum` case already is.
#[test]
fn an_empty_record_has_no_component_type() {
    expect_bad("record-empty.wit", &["E-RECORD-EMPTY"]);

    // Reported wherever it is reachable -- the declaration in the world, the
    // parameter and the result -- because each is a place the component type
    // would have to be written.
    let got: Vec<_> = report(&fixture("bad", "record-empty.wit"))
        .rejections
        .into_iter()
        .map(|r| (r.reason, r.site))
        .collect();
    assert!(
        got.iter()
            .all(|(reason, _)| *reason == support::Reason::Malformed),
        "{got:?}"
    );
    assert!(
        got.iter().any(|(_, site)| site.contains("param `n`")),
        "{got:?}"
    );
    assert!(
        got.iter().any(|(_, site)| site.contains("result")),
        "{got:?}"
    );
}

/// The other refusal that is about the record rather than its fields, and the
/// one `checks/backend.py` found: a record with exactly one field. The
/// backend's erasure unboxes a single-constructor inductive of one argument,
/// so the value *is* the field and the projection is the identity, while the
/// emitters build a block and read a cell out of it. Against an unboxed value
/// that is a wrong answer rather than a trap (TODO.md#record-unboxed).
///
/// Refused at every depth, because a one-field record nested in another is
/// the shape that produced the wrong answer in the first place.
#[test]
fn a_one_field_record_is_not_a_block() {
    expect_bad("record-single.wit", &["E-RECORD-UNBOXED"]);

    let nested: Vec<_> = report(&fixture("bad", "record-single-nested.wit"))
        .rejections
        .into_iter()
        .map(|r| (r.code, r.site))
        .collect();
    assert!(
        nested.iter().all(|(code, _)| *code == "E-RECORD-UNBOXED"),
        "{nested:?}"
    );
    // Named by the field it is a field of, which is the line to change --
    // and in both positions, since `outer` is the parameter and the result.
    assert!(
        nested
            .iter()
            .any(|(_, site)| site.contains("param `o`, field `i`")),
        "{nested:?}"
    );
    assert!(
        nested
            .iter()
            .any(|(_, site)| site.contains("result, field `i`")),
        "{nested:?}"
    );
}

#[test]
fn rejects_imports_and_namespaced_exports() {
    expect_bad("import.wit", &["E-IMPORT"]);
    expect_bad(
        "exported-interface.wit",
        &["E-EXPORT-INTERFACE", "E-NO-EXPORT"],
    );
}

/// Every offending position is named, not just the first one: `f64` in the
/// parameter and `f32` in the result are two findings.
#[test]
fn reports_every_offending_position() {
    let got: Vec<_> = report(&fixture("bad", "float.wit"))
        .rejections
        .into_iter()
        .map(|r| (r.what, r.site))
        .collect();
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got
        .iter()
        .any(|(what, site)| what == "f64" && site.contains("param")));
    assert!(got
        .iter()
        .any(|(what, site)| what == "f32" && site.contains("result")));
}

/// The one exported function of a fixture.
fn export_of(path: &Path) -> (wit_parser::Resolve, wit_parser::Function) {
    let (resolve, world) = resolve_world(path, None).unwrap();
    let f = resolve.worlds[world]
        .exports
        .values()
        .find_map(|item| match item {
            wit_parser::WorldItem::Function(f) => Some(f.clone()),
            _ => None,
        })
        .expect("one exported function");
    (resolve, f)
}

/// What a sum contributes to the failure list, which is decided by what it
/// *carries* rather than by what it is. An `option<u64>` can hold a `u64` with
/// no `Uint63.int`; an `option<bool>` cannot hold anything of the sort, and a
/// failure listed for it would be one the adapter has no branch for -- the
/// generated `interface.v` names the failures from this list, so an invented
/// one ends up in the user's file.
#[test]
fn only_a_sum_that_carries_a_u64_can_be_out_of_range() {
    let (resolve, f) = export_of(&fixture("good", "option.wit"));
    assert_eq!(
        support::failure_modes(&resolve, &f),
        vec![
            support::Failure::OutOfMemory,
            // Named as the payload it is, not as the parameter: `x` itself is
            // a sum and is never out of range.
            support::Failure::OutOfRange("a `u64` inside `x` >= 2^63".into()),
        ]
    );

    let (resolve, f) = export_of(&fixture("good", "option-bool.wit"));
    assert_eq!(
        support::failure_modes(&resolve, &f),
        vec![support::Failure::OutOfMemory],
        "nothing an `option<bool>` holds has to narrow"
    );

    // Two `u64`s in one sum is still one failure mode: the list is the ways a
    // call can fail, and both halves fail the same way. The adapter emits a
    // check per case -- `both_halves_of_a_sum_are_checked_separately`.
    let (resolve, f) = export_of(&fixture("good", "nested-mixed.wit"));
    assert_eq!(
        support::failure_modes(&resolve, &f),
        vec![
            support::Failure::OutOfMemory,
            support::Failure::OutOfRange("a `u64` inside `x` >= 2^63".into()),
        ]
    );
}

/// u64 is carried by a 63-bit integer, so lowering one can overflow. That is
/// accounted for as a failure mode of the parameter -- not of the result,
/// where lifting a 63-bit int into a u64 is always exact -- and reported
/// through the error channel rather than as a warning.
#[test]
fn a_u64_parameter_is_a_failure_mode() {
    use componentize_formal::resolve_world;
    use wit_parser::WorldItem;

    let path = fixture("good", "scalars.wit");
    let (resolve, world) = resolve_world(&path, None).unwrap();

    let f = resolve.worlds[world]
        .exports
        .values()
        .find_map(|item| match item {
            WorldItem::Function(f) => Some(f),
            _ => None,
        })
        .expect("one exported function");

    assert_eq!(
        support::failure_modes(&resolve, f),
        vec![
            support::Failure::OutOfMemory,
            support::Failure::OutOfRange("`val` >= 2^63".into()),
        ]
    );
    // A clean world with a proper channel produces no warnings at all.
    assert!(report(&path).warnings.is_empty());

    // The converse, which is the half that could go wrong quietly: a `u64` in
    // the *result* and none in the parameters cannot be out of range, so the
    // only failure is out of memory. A range check derived from "the world
    // mentions u64" rather than from the parameters would show up here, and
    // the adapter side of it is `a_u64_result_needs_no_range_check`.
    let widen = fixture("good", "widen.wit");
    let (resolve, world) = resolve_world(&widen, None).unwrap();
    let f = resolve.worlds[world]
        .exports
        .values()
        .find_map(|item| match item {
            WorldItem::Function(f) => Some(f),
            _ => None,
        })
        .expect("one exported function");
    assert_eq!(
        support::failure_modes(&resolve, f),
        vec![support::Failure::OutOfMemory]
    );
}

/// A code cannot be added without documenting it.
#[test]
fn every_rejection_points_at_a_todo_section() {
    let todo = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../TODO.md"))
        .expect("TODO.md");

    let mut checked = 0;
    for path in wit_files("bad") {
        for rejection in report(&path).rejections {
            assert!(
                todo.contains(&format!("<a id=\"{}\"></a>", rejection.anchor)),
                "{}: TODO.md has no anchor `{}`",
                path.display(),
                rejection.anchor
            );
            assert!(
                todo.contains(rejection.code),
                "{}: TODO.md never mentions `{}`",
                path.display(),
                rejection.code
            );
            checked += 1;
        }
    }
    assert!(checked > 10, "only {checked} rejections exercised");
}

/// Every export must be able to report every way it can fail. The ways a
/// result type fails to do that are reported separately, because the fix
/// differs.
#[test]
fn requires_an_error_channel() {
    expect_bad("no-channel.wit", &["E-NO-CHANNEL"]);
    expect_bad("option-channel.wit", &["E-NO-CHANNEL"]);
    expect_bad("no-error-half.wit", &["E-NO-CHANNEL"]);
    expect_bad("error-type.wit", &["E-ERROR-TYPE"]);
}

/// A look-alike enum of the user's own is not the shipped error type, however
/// well its cases would line up.
#[test]
fn the_error_type_is_matched_by_identity() {
    let report = report(&fixture("bad", "error-type.wit"));
    let rejection = &report.rejections[0];
    assert_eq!(rejection.code, "E-ERROR-TYPE");
    // Spelled from the shipped package so a version bump cannot leave this
    // asserting the old name, and spelled the way WIT spells it -- the version
    // attaches to the interface, so a reader can copy it into a `use`.
    let want = format!("{}.error", componentize_formal::abi_use_path());
    assert!(rejection.note.contains(&want), "{}", rejection.note);
}

/// The failure modes a signature can produce are named, so the shortfall is
/// visible rather than left to be inferred.
#[test]
fn names_the_failure_modes() {
    let report = report(&fixture("bad", "option-channel.wit"));
    let note = &report.rejections[0].note;
    assert!(note.contains("out of memory"), "{note}");
    assert!(note.contains("`val` >= 2^63"), "{note}");
}

/// The shipped package resolves without the world vendoring a deps/ tree.
#[test]
fn the_shipped_abi_package_is_available() {
    assert!(componentize_formal::ABI_WIT.contains("package certirocq:abi@"));
    // good/scalars.wit `use`s it; resolving at all proves it is reachable.
    assert!(report(&fixture("good", "scalars.wit")).is_ok());
}

/// The `error` enum is versioned, and the version is part of its identity: an
/// enum's case order is its discriminant in the canonical ABI, so an `error`
/// from another version of the package is a different type. A world asking for
/// one must be told which version is shipped, not left with a bare "package
/// not found".
#[test]
fn the_abi_package_is_versioned() {
    let shipped = componentize_formal::abi_package_name();
    assert!(shipped.version.is_some(), "{shipped}");

    let dir = std::env::temp_dir().join(format!("bindgen-abi-version-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("skew.wit");
    std::fs::write(
        &path,
        "package local:skew;\nworld w {\n  \
         use certirocq:abi/errors@9.9.9.{error};\n  \
         export plus1: func(val: u64) -> result<u64, error>;\n}\n",
    )
    .unwrap();

    let err = format!(
        "{:#}",
        resolve_world(&path, None).expect_err("a version we do not ship must not resolve")
    );
    assert!(err.contains(&shipped.to_string()), "{err}");
    assert!(err.contains("TODO.md#abi-package-version"), "{err}");

    let todo = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../TODO.md"))
        .expect("TODO.md");
    assert!(todo.contains("<a id=\"abi-package-version\"></a>"));

    std::fs::remove_dir_all(&dir).ok();
}

/// Rocq is the default and the only implemented frontend. Selecting another
/// must fail: generating Rocq artefacts for someone who asked for Lean would be
/// precisely the silent failure this tool is built to avoid.
#[test]
fn only_the_rocq_frontend_is_implemented() {
    use componentize_formal::config::Frontend;

    assert_eq!(Frontend::default(), Frontend::Rocq);
    assert!(Frontend::Rocq.ensure_supported().is_ok());

    let err = Frontend::Lean
        .ensure_supported()
        .expect_err("lean must be rejected")
        .to_string();
    assert!(err.contains("not implemented yet"), "{err}");
    // Held to the same standard as the WIT rejections: it points somewhere.
    assert!(err.contains("TODO.md#frontends"), "{err}");

    let todo = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../TODO.md"))
        .expect("TODO.md");
    assert!(todo.contains("<a id=\"frontends\"></a>"));
}

/// The frontend is selectable from the config file, not only the flag.
#[test]
fn the_config_selects_a_frontend() {
    use componentize_formal::config::{Config, Frontend};

    let dir = std::env::temp_dir().join(format!("bindgen-frontend-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let path = dir.join("component.toml");
    std::fs::write(&path, "wit = \"world.wit\"\n").unwrap();
    assert_eq!(Config::load(&path).unwrap().frontend, Frontend::Rocq);

    std::fs::write(&path, "wit = \"world.wit\"\nfrontend = \"lean\"\n").unwrap();
    let cfg = Config::load(&path).unwrap();
    assert_eq!(cfg.frontend, Frontend::Lean);
    assert!(cfg.frontend.ensure_supported().is_err());

    std::fs::remove_dir_all(&dir).ok();
}

/// A world laid out as a WIT directory carries `certirocq:abi` under
/// `deps/`, because that copy is what anything resolving the directory
/// reads. Pushing the shipped one as well used to make `wit-parser` panic on
/// a duplicate package, so the vendored layout has to resolve on its own.
#[test]
fn a_vendored_abi_package_resolves() {
    let dir = std::env::temp_dir().join(format!("bindgen-vendored-{}", std::process::id()));
    let deps = dir.join("deps/certirocq-abi");
    std::fs::create_dir_all(&deps).unwrap();
    std::fs::write(deps.join("errors.wit"), componentize_formal::ABI_WIT).unwrap();
    std::fs::write(
        dir.join("world.wit"),
        format!(
            "package local:vendored;\nworld w {{\n  use {}.{{error}};\n  \
             export plus1: func(val: u64) -> result<u64, error>;\n}}\n",
            componentize_formal::abi_use_path()
        ),
    )
    .unwrap();

    let (resolve, world) = resolve_world(&dir, None)
        .unwrap_or_else(|e| panic!("a vendored deps/ layout must resolve: {e:#}"));
    let report = support::check_world(&resolve, world);
    assert!(
        report.is_ok(),
        "{}",
        report
            .rejections
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// A vendored copy is trusted for its name by the gate, so its contents have
/// to be checked here. Same version, different case order means the adapter
/// writes one discriminant while the component declares another -- the exact
/// silent failure the version is supposed to prevent.
#[test]
fn a_vendored_abi_package_must_match_the_shipped_one() {
    let shipped = componentize_formal::abi_package_name();
    let use_path = componentize_formal::abi_use_path();
    let dir = std::env::temp_dir().join(format!("bindgen-vendored-skew-{}", std::process::id()));
    let deps = dir.join("deps/certirocq-abi");
    std::fs::create_dir_all(&deps).unwrap();

    // The shipped cases in the other order: same name, same version, different
    // discriminants.
    std::fs::write(
        deps.join("errors.wit"),
        format!(
            "package {shipped};\ninterface errors {{\n  \
             enum error {{ out-of-range, out-of-memory }}\n}}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("world.wit"),
        format!(
            "package local:skewed;\nworld w {{\n  use {use_path}.{{error}};\n  \
             export plus1: func(val: u64) -> result<u64, error>;\n}}\n"
        ),
    )
    .unwrap();

    let err = format!(
        "{:#}",
        resolve_world(&dir, None).expect_err("reordered cases are a different type")
    );
    assert!(err.contains("out-of-range"), "{err}");
    assert!(err.contains("TODO.md#abi-package-version"), "{err}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Everything else that resolves a world -- this tool's next run,
/// `wasm-tools component wit` -- reads the *directory*, and
/// `Resolve::push_path` on a single file does not even read the `deps/` beside
/// it. So the two can see different copies of `certirocq:abi`, and the copy
/// that matters is the one on disk: without this check a world passed as a file
/// would be gated against the shipped package while a later resolve sees a vendored one
/// whose cases had moved, producing a component that reports the wrong error.
#[test]
fn a_world_file_is_held_to_the_package_beside_it() {
    let dir = std::env::temp_dir().join(format!("bindgen-beside-{}", std::process::id()));
    let deps = dir.join("deps/certirocq-abi");
    std::fs::create_dir_all(&deps).unwrap();
    std::fs::write(
        deps.join("errors.wit"),
        "package certirocq:abi@0.1.0;\n\
         interface errors {\n\
         \x20 enum error { out-of-range, out-of-memory }\n\
         }\n",
    )
    .unwrap();
    let world = dir.join("world.wit");
    std::fs::write(
        &world,
        "package local:demo;\n\
         world w {\n\
         \x20 use certirocq:abi/errors@0.1.0.{error};\n\
         \x20 export plus1: func(val: u64) -> result<u64, error>;\n\
         }\n",
    )
    .unwrap();

    let err = format!(
        "{:#}",
        resolve_world(&world, None).expect_err("the skewed copy beside it must be caught")
    );
    assert!(err.contains("out-of-range"), "{err}");
    assert!(err.contains("TODO.md#abi-package-version"), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A tree that vendors the package and has an ordinary mistake in its world
/// file must report the mistake.
///
/// This used to be a panic from inside `wit-parser`. Resolution tried the tree
/// first and pushed the shipped package as a fallback, so the parse error sent
/// it down the fallback, where pushing our copy on top of the vendored one is
/// a duplicate package -- which that crate panics on rather than returning. A
/// misplaced comma came out as a crash.
#[test]
fn a_mistake_in_a_vendored_tree_is_reported_not_panicked() {
    let dir = std::env::temp_dir().join(format!("bindgen-typo-{}", std::process::id()));
    let deps = dir.join("deps/certirocq-abi");
    std::fs::create_dir_all(&deps).unwrap();
    std::fs::write(deps.join("errors.wit"), componentize_formal::ABI_WIT).unwrap();
    std::fs::write(
        dir.join("world.wit"),
        "package local:demo;\n\
         world w {\n\
         \x20 use certirocq:abi/errors@0.1.0.{error};\n\
         \x20 export plus1 func(val: u64) -> result<u64, error>;\n\
         }\n",
    )
    .unwrap();

    let err = format!(
        "{:#}",
        resolve_world(&dir, None).expect_err("a syntax error must be an error")
    );
    assert!(err.contains("expected ';'"), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A type with no cases must not reach the gate.
///
/// `enum nothing { }` and `variant nothing { }` are the only WIT spellings of
/// an uninhabited type, and every claim `repr.rs` makes is about a constructor
/// there is at least one of. A case-less enum would plan as `Repr::Enum {
/// cases: 0, .. }`, declare `Inductive nothing := .` in `interface.v`, and
/// leave the adapter lowering a discriminant no value of the type has -- an
/// export nothing could ever call.
///
/// Nothing in the emitters or the gate says so, because `wit-parser` refuses
/// both while parsing, before `resolve_world` returns a world there would be
/// anything to walk. That is why neither has an `E-` code, and it is also why
/// this test exists: the refusal belongs to the pinned parser rather than to
/// us, so a bump that started accepting an empty enum would hand one straight
/// to the emitters with nothing in between.
#[test]
fn a_type_with_no_cases_is_refused_by_the_parser() {
    let refused = |label: &str, decl: &str| -> String {
        let dir = std::env::temp_dir().join(format!("bindgen-{label}-{}", std::process::id()));
        let deps = dir.join("deps/certirocq-abi");
        std::fs::create_dir_all(&deps).unwrap();
        std::fs::write(deps.join("errors.wit"), componentize_formal::ABI_WIT).unwrap();
        std::fs::write(
            dir.join("world.wit"),
            format!(
                "package local:demo;\n\
                 world w {{\n\
                 \x20 use certirocq:abi/errors@0.1.0.{{error}};\n\
                 \x20 {decl}\n\
                 \x20 export pick: func(n: nothing) -> result<u64, error>;\n\
                 }}\n"
            ),
        )
        .unwrap();
        let err = format!(
            "{:#}",
            resolve_world(&dir, None).expect_err("a type with no cases must be an error")
        );
        std::fs::remove_dir_all(&dir).ok();
        err
    };

    let no_enum_cases = refused("empty-enum", "enum nothing { }");
    assert!(no_enum_cases.contains("empty enum"), "{no_enum_cases}");

    let no_variant_cases = refused("empty-variant", "variant nothing { }");
    assert!(
        no_variant_cases.contains("empty variant"),
        "{no_variant_cases}"
    );
}

/// Every code in the catalogue has a `TODO.md` section saying what it would
/// take to lift, and every code the gate raises is in the catalogue.
///
/// The gate's module doc has always said to keep the two in sync; this is that
/// sentence as a check. The second direction is the one that rots: a refusal
/// moved out of the gate -- `E-ARITY-FLAT` lives in the planner, because only
/// the canonical-ABI layout knows the flat count -- keeps working while
/// quietly leaving the catalogue, and then nothing documents it.
#[test]
fn every_code_is_documented() {
    let todo = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("TODO.md"),
    )
    .unwrap();
    let undocumented: Vec<_> = componentize_formal::codes::ALL
        .iter()
        .filter(|code| !todo.contains(**code))
        .collect();
    assert!(
        undocumented.is_empty(),
        "codes with no TODO.md section: {undocumented:?}"
    );

    let mut raised = BTreeSet::new();
    for kind in ["bad", "good"] {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/wit")
            .join(kind);
        for entry in std::fs::read_dir(dir).unwrap() {
            raised.extend(codes(&entry.unwrap().path()));
        }
    }
    let stray: Vec<_> = raised
        .iter()
        .filter(|code| !componentize_formal::codes::ALL.contains(&code.as_str()))
        .collect();
    assert!(
        stray.is_empty(),
        "codes raised by a fixture but missing from codes::ALL: {stray:?}"
    );
}
