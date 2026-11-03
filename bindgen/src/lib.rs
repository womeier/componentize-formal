//! Generate a Rocq interface, a CertiRocq extraction file and canonical-ABI
//! glue from a WIT world.

pub mod config;
pub mod repr;
pub mod support;

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::OnceLock;
use wit_parser::{PackageName, Resolve, UnresolvedPackageGroup, WorldId};

/// The `certirocq:abi` package, shipped with the generator so a world can
/// `use certirocq:abi/errors@0.1.0.{error}` without vendoring anything.
pub const ABI_WIT: &str = include_str!("../wit/certirocq-abi.wit");

/// Interface and type name of the error enum every export returns. The
/// package name is not restated here -- see `abi_package_name`.
pub const ABI_INTERFACE: &str = "errors";
pub const ABI_ERROR: &str = "error";

/// The shipped package's name, version included, read off the shipped file so
/// the two cannot drift.
///
/// The version is part of the type's identity, not decoration: an `enum`'s
/// case order is its discriminant in the canonical ABI, so adding or moving a
/// case changes what the adapter's `i32` means. An `error` from some other
/// version of `certirocq:abi` is a different type, and the gate has to say so
/// rather than populate it with cases that happen to line up.
pub fn abi_package_name() -> &'static PackageName {
    static NAME: OnceLock<PackageName> = OnceLock::new();
    NAME.get_or_init(|| {
        parse_abi()
            .expect("the shipped certirocq:abi package must parse")
            .main
            .name
            .clone()
    })
}

fn parse_abi() -> Result<UnresolvedPackageGroup> {
    UnresolvedPackageGroup::parse("certirocq-abi.wit", ABI_WIT)
        .map_err(|(_, e)| e)
        .context("parsing the shipped certirocq:abi package")
}

/// Load a WIT file or directory and pick the world to work on.
pub fn resolve_world(wit: &Path, world: Option<&str>) -> Result<(Resolve, WorldId)> {
    let mut resolve = Resolve::new();

    // Pushed first so the user's `use certirocq:abi/errors@0.1.0.{error}`
    // resolves against it instead of needing a deps/ directory.
    resolve
        .push_group(parse_abi()?)
        .context("loading the shipped certirocq:abi package")?;

    let (pkg, _files) = resolve.push_path(wit).map_err(|e| abi_hint(e, wit))?;
    let world = resolve
        .select_world(&[pkg], world)
        .context("selecting a world")?;
    Ok((resolve, world))
}

/// Resolution failures that mention `certirocq:abi` are nearly always a
/// version skew -- the shipped package is the only one of that name in the
/// resolve, so a world asking for another version simply fails to resolve.
/// A bare "package not found" leaves the reader guessing which one is here.
fn abi_hint(err: anyhow::Error, wit: &Path) -> anyhow::Error {
    let shown = format!("{err:#}");
    let err = err.context(format!("loading {}", wit.display()));
    if shown.contains("certirocq:abi") {
        let shipped = abi_package_name();
        return err.context(format!(
            "this generator ships `{shipped}`; a world must `use {shipped}/{}.{{{}}}` \
             with that exact version -- see TODO.md#abi-package-version",
            ABI_INTERFACE, ABI_ERROR
        ));
    }
    err
}
