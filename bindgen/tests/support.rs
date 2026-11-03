//! Pin every fixture to the codes it should produce.
//!
//! Two structural guarantees matter more than the individual cases: nothing
//! under `wit/bad` may ever pass the gate, and nothing under `wit/good` may
//! ever fail it. Either direction would be a silent failure -- an adapter
//! generated for a world the backend cannot represent, or a usable world turned
//! away.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rocq_component_bindgen::{resolve_world, support};

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
    expect_bad("nested-option.wit", &["E-STRUCTURED"]);
    expect_bad("arity.wit", &["E-ARITY-N"]);
    expect_bad("nullary.wit", &["E-ARITY-0"]);
    expect_bad("no-result.wit", &["E-NO-RESULT"]);
    expect_bad("multi-export.wit", &["E-MULTI-EXPORT"]);
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

/// A result type with nowhere to put an out-of-memory signal is accepted, but
/// never quietly: the adapter will trap, and the user is told so.
#[test]
fn warns_when_there_is_no_oom_channel() {
    let report = report(&fixture("good", "no-oom-channel.wit"));
    assert!(report.is_ok());
    assert!(
        report.warnings.iter().any(|w| w.contains("out-of-memory")),
        "expected an OOM warning, got {:?}",
        report.warnings
    );
}

/// u64 is carried by a 63-bit integer. That narrowing must be announced.
#[test]
fn warns_about_the_u64_range() {
    let report = report(&fixture("good", "scalars.wit"));
    assert!(report.is_ok());
    assert_eq!(
        report.warnings.len(),
        2,
        "expected the parameter and the result to be flagged: {:?}",
        report.warnings
    );
    assert!(report.warnings.iter().all(|w| w.contains("2^63")));
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
