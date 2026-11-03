//! Generate a Rocq interface, a CertiRocq extraction file and canonical-ABI
//! glue from a WIT world.

pub mod config;
pub mod repr;
pub mod support;

use anyhow::{Context, Result};
use std::path::Path;
use wit_parser::{Resolve, UnresolvedPackageGroup, WorldId};

/// The `certirocq:abi` package, shipped with the generator so a world can
/// `use certirocq:abi/errors.{error}` without vendoring anything.
pub const ABI_WIT: &str = include_str!("../wit/certirocq-abi.wit");

/// Package, interface and type name of the error enum every export returns.
pub const ABI_PACKAGE: (&str, &str) = ("certirocq", "abi");
pub const ABI_INTERFACE: &str = "errors";
pub const ABI_ERROR: &str = "error";

/// Load a WIT file or directory and pick the world to work on.
pub fn resolve_world(wit: &Path, world: Option<&str>) -> Result<(Resolve, WorldId)> {
    let mut resolve = Resolve::new();

    // Pushed first so the user's `use certirocq:abi/errors.{error}` resolves
    // against it instead of needing a deps/ directory.
    let abi = UnresolvedPackageGroup::parse("certirocq-abi.wit", ABI_WIT)
        .map_err(|(_, e)| e)
        .context("parsing the shipped certirocq:abi package")?;
    resolve
        .push_group(abi)
        .context("loading the shipped certirocq:abi package")?;

    let (pkg, _files) = resolve
        .push_path(wit)
        .with_context(|| format!("loading {}", wit.display()))?;
    let world = resolve
        .select_world(&[pkg], world)
        .context("selecting a world")?;
    Ok((resolve, world))
}
