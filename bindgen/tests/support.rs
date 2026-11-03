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
    expect_bad("structured.wit", &["E-STRUCTURED"]);
    // option and result are supported where they carry the out-of-memory
    // signal; the other positions need lowering into the heap.
    expect_bad("nested-option.wit", &["E-OPTION"]);
    expect_bad("option-param.wit", &["E-OPTION"]);
    expect_bad("arity.wit", &["E-ARITY-N"]);
    expect_bad("nullary.wit", &["E-ARITY-0"]);
    expect_bad("no-result.wit", &["E-NO-RESULT"]);
    expect_bad("multi-export.wit", &["E-MULTI-EXPORT"]);
    // u64 is the only integer width carried so far.
    for width in ["u8", "u16", "u32"] {
        expect_bad(&format!("int-{width}.wit"), &["E-INT-WIDTH"]);
    }
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
    // asserting the old name.
    let want = format!("{}/errors.error", componentize_formal::abi_package_name());
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
