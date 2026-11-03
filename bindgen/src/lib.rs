//! Generate a Rocq interface, a CertiRocq extraction file and canonical-ABI
//! glue from a WIT world.

pub mod config;
pub mod support;

use anyhow::{Context, Result};
use std::path::Path;
use wit_parser::{Resolve, WorldId};

/// Load a WIT file or directory and pick the world to work on.
pub fn resolve_world(wit: &Path, world: Option<&str>) -> Result<(Resolve, WorldId)> {
    let mut resolve = Resolve::new();
    let (pkg, _files) = resolve
        .push_path(wit)
        .with_context(|| format!("loading {}", wit.display()))?;
    let world = resolve
        .select_world(&[pkg], world)
        .context("selecting a world")?;
    Ok((resolve, world))
}
