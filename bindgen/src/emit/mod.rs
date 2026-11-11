//! Turn a checked world into the files that build a component.
//!
//! The split is deliberate: [`plan`] answers every question and can fail,
//! [`rocq`], [`adapter`] and [`build`] format the answers and cannot. So a run
//! either writes a set of files that agree with each other or writes nothing.
//!
//! [`convert`] sits under the emitters rather than beside them: it holds the
//! two conversions between CertiRocq's values and the canonical ABI's, in both
//! directions, so an export's glue and an import wrapper's are the same pair
//! used in opposite orders.
//!
//! [`roundtrip`] is not in [`artifacts`] and a component build never sees it.
//! It emits a module that composes each of those conversions both ways so that
//! `checks/roundtrip.py` can run them and require the value back unchanged --
//! the one direction an export adapter never exercises is the one an import
//! wrapper will be built on (TODO.md#imports).

pub mod adapter;
pub mod build;
pub mod convert;
pub mod plan;
pub mod rocq;
pub mod roundtrip;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

pub use plan::Plan;

/// What every generated file says about itself, and what the writer looks for
/// before overwriting one. A file without it is taken to be hand-written, and
/// hand-written work is not something a generator gets to destroy.
pub const MARKER: &str = "componentize-formal generate";

/// The module name `wasm-merge` is given for the compiled Rocq module, and so
/// the module every import in the adapter is written against.
pub const BACKEND_MODULE: &str = "rocq";

/// Whether an existing file is this generator's to rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    /// Regenerated on every run.
    Always,
    /// Written once and then left alone, because it is the user's to edit --
    /// or, in the vendored package's case, because the build reads it and a
    /// local change may be deliberate.
    IfMissing,
}

pub struct Artifact {
    /// Where it goes, relative to the output directory.
    pub path: PathBuf,
    pub contents: String,
    pub overwrite: Overwrite,
    /// One line on what it is for, printed when it is written.
    pub what: &'static str,
}

/// Prose as a comment block, one line per line.
///
/// Worth three functions rather than string literals: a `\` continuation in a
/// Rust literal eats the next line's leading whitespace, which silently
/// un-indents the ` *` column of a Rocq comment and the `;;` of a wat one. The
/// generated files looked like a mistake before this existed.
pub mod comment {
    /// `(* ... *)`, with the continuation column Rocq files conventionally use.
    pub fn rocq<S: AsRef<str>>(lines: &[S]) -> String {
        block(lines, "(*", " *", Some(" *)"))
    }

    /// `;; ...`, indented to sit with the code it describes.
    pub fn wat<S: AsRef<str>>(indent: &str, lines: &[S]) -> String {
        let head = format!("{indent};;");
        block(lines, &head, &head, None)
    }

    /// `# ...`.
    pub fn make<S: AsRef<str>>(lines: &[S]) -> String {
        block(lines, "#", "#", None)
    }

    fn block<S: AsRef<str>>(lines: &[S], first: &str, rest: &str, close: Option<&str>) -> String {
        let mut out = String::new();
        for (i, line) in lines.iter().enumerate() {
            let head = if i == 0 { first } else { rest };
            let tail = match (close, i + 1 == lines.len()) {
                (Some(close), true) => close,
                _ => "",
            };
            let line = line.as_ref();
            // An empty line keeps the marker and nothing else: trailing
            // whitespace in a generated file is noise in every diff of it.
            match line.is_empty() && tail.is_empty() {
                true => out.push_str(head),
                false => out.push_str(&format!("{head} {line}{tail}")),
            }
            out.push('\n');
        }
        out
    }
}

/// What happened to one artifact.
#[derive(Debug)]
pub enum Wrote {
    Written(PathBuf),
    Kept(PathBuf),
}

impl Wrote {
    pub fn path(&self) -> &Path {
        match self {
            Wrote::Written(path) | Wrote::Kept(path) => path,
        }
    }
}

/// Every file a world needs to become a component, except `impl.v`.
pub fn artifacts(plan: &Plan) -> Vec<Artifact> {
    vec![
        Artifact {
            path: PathBuf::from(format!("{}.v", plan.build.interface_stem)),
            contents: rocq::interface(plan),
            overwrite: Overwrite::Always,
            what: "the world as a Rocq Module Type",
        },
        Artifact {
            path: PathBuf::from(format!("{}.v", plan.build.extraction_stem)),
            contents: rocq::extraction(plan),
            overwrite: Overwrite::Always,
            what: "the CertiRocq Compile Wasm invocation",
        },
        Artifact {
            path: PathBuf::from("adapter.wat"),
            contents: adapter::adapter(plan),
            overwrite: Overwrite::Always,
            what: "canonical-ABI glue",
        },
        Artifact {
            path: PathBuf::from("Makefile"),
            contents: build::makefile(plan),
            overwrite: Overwrite::Always,
            what: "build rules: rocq c, wasm-merge, component embed, component new",
        },
    ]
}

/// The shipped `certirocq:abi` package, vendored into the world's `deps/`.
///
/// Not decoration: `wasm-tools component embed` resolves the world from the
/// WIT directory alone, with no idea that this generator ships a package. A
/// world that `use`s the error enum therefore does not build until the package
/// is on disk next to it, and the failure comes out of `wasm-tools` as an
/// unresolved package rather than from anything that could explain itself.
pub fn vendored_abi(plan: &Plan) -> Artifact {
    Artifact {
        path: Path::new(&plan.build.wit_dir)
            .join("deps")
            .join(format!(
                "{}-{}",
                crate::abi_package_name().namespace,
                crate::abi_package_name().name
            ))
            .join(format!("{}.wit", crate::ABI_INTERFACE)),
        contents: crate::ABI_WIT.to_string(),
        overwrite: Overwrite::IfMissing,
        what: "the shipped error enum, where `component embed` will look for it",
    }
}

/// Write the artifacts into `dir`, refusing to clobber anything hand-written.
pub fn write(dir: &Path, artifacts: &[Artifact], force: bool) -> Result<Vec<Wrote>> {
    // Checked before anything is written, so a refusal leaves the directory as
    // it was rather than half rewritten.
    for artifact in artifacts {
        let path = dir.join(&artifact.path);
        if artifact.overwrite == Overwrite::IfMissing || !path.exists() || force {
            continue;
        }
        let existing = fs::read_to_string(&path)
            .with_context(|| format!("reading the existing {}", path.display()))?;
        if !existing.contains(MARKER) {
            bail!(
                "{} exists and does not look generated: it has no `{MARKER}` line, so \
                 it may be yours. Nothing was written. Move it aside, or pass --force \
                 to overwrite it.",
                path.display()
            );
        }
    }

    let mut done = Vec::new();
    for artifact in artifacts {
        let path = dir.join(&artifact.path);
        if artifact.overwrite == Overwrite::IfMissing && path.exists() {
            done.push(Wrote::Kept(artifact.path.clone()));
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&path, &artifact.contents)
            .with_context(|| format!("writing {}", path.display()))?;
        done.push(Wrote::Written(artifact.path.clone()));
    }
    Ok(done)
}
