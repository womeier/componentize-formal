//! Generate a Rocq interface, a CertiRocq extraction file and canonical-ABI
//! glue from a WIT world.

pub mod config;
pub mod measured;
pub mod repr;
pub mod support;

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::sync::OnceLock;
use wit_parser::{PackageName, Resolve, TypeDefKind, UnresolvedPackageGroup, WorldId};

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

/// How a world has to spell the shipped error type, which is *not* how a
/// package name is printed: the version attaches to the interface, so it is
/// `certirocq:abi/errors@0.1.0`, not `certirocq:abi@0.1.0/errors`. Only one of
/// those parses, and an error message that hands back the other is worse than
/// one that says nothing.
pub fn abi_use_path() -> String {
    let pkg = abi_package_name();
    match &pkg.version {
        Some(version) => format!(
            "{}:{}/{}@{}",
            pkg.namespace, pkg.name, ABI_INTERFACE, version
        ),
        None => format!("{}:{}/{}", pkg.namespace, pkg.name, ABI_INTERFACE),
    }
}

fn parse_abi() -> Result<UnresolvedPackageGroup> {
    UnresolvedPackageGroup::parse("certirocq-abi.wit", ABI_WIT)
        .map_err(|(_, e)| e)
        .context("parsing the shipped certirocq:abi package")
}

/// The shipped `error`'s case names, in declaration order --- which *is* the
/// canonical-ABI discriminant, so the order is as load-bearing as the names.
fn abi_error_cases() -> &'static [String] {
    static CASES: OnceLock<Vec<String>> = OnceLock::new();
    CASES.get_or_init(|| {
        let mut resolve = Resolve::new();
        resolve
            .push_group(parse_abi().expect("the shipped certirocq:abi package must parse"))
            .expect("the shipped certirocq:abi package must resolve");
        error_cases(&resolve, abi_package_name()).expect("the shipped `error` must be an enum")
    })
}

/// The cases of `<pkg>/errors.error`, in order.
fn error_cases(resolve: &Resolve, pkg: &PackageName) -> Result<Vec<String>> {
    let id = *resolve
        .package_names
        .get(pkg)
        .with_context(|| format!("`{pkg}` is not in this resolve"))?;
    let iface = *resolve.packages[id]
        .interfaces
        .get(ABI_INTERFACE)
        .with_context(|| format!("`{pkg}` has no interface `{ABI_INTERFACE}`"))?;
    let ty = *resolve.interfaces[iface]
        .types
        .get(ABI_ERROR)
        .with_context(|| format!("`{pkg}/{ABI_INTERFACE}` has no type `{ABI_ERROR}`"))?;
    match &resolve.types[ty].kind {
        TypeDefKind::Enum(e) => Ok(e.cases.iter().map(|c| c.name.clone()).collect()),
        kind => bail!("`{pkg}/{ABI_INTERFACE}.{ABI_ERROR}` is {kind:?}, not an enum"),
    }
}

/// Load a WIT file or directory and pick the world to work on.
///
/// Two layouts have to work, and they want opposite things. A bare world file
/// needs the shipped `certirocq:abi` pushed under it, so
/// `use certirocq:abi/errors@0.1.0.{error}` resolves with nothing vendored. A
/// directory laid out for `wasm-tools component embed` already carries the
/// package under `deps/` --- it has to, since that copy is what the component
/// build reads --- and pushing ours as well is a duplicate package, which
/// `wit-parser` panics on rather than reporting.
///
/// So the self-contained tree is tried first and the shipped package is a
/// fallback, which also means a vendored copy always wins over ours: it is the
/// one the component will be built against.
pub fn resolve_world(wit: &Path, world: Option<&str>) -> Result<(Resolve, WorldId)> {
    let mut resolve = Resolve::new();
    let pkg = match resolve.push_path(wit) {
        Ok((pkg, _files)) => {
            // Trusted for its name by the gate, so held to its contents here:
            // the same version with different cases would mean the adapter
            // writing one discriminant while the component declares another.
            check_vendored_abi(&resolve, wit)?;
            pkg
        }
        Err(_) => {
            resolve = Resolve::new();
            resolve
                .push_group(parse_abi()?)
                .context("loading the shipped certirocq:abi package")?;
            let (pkg, _files) = resolve.push_path(wit).map_err(|e| abi_hint(e, wit))?;
            pkg
        }
    };

    let world = resolve
        .select_world(&[pkg], world)
        .context("selecting a world")?;
    Ok((resolve, world))
}

/// Hold a vendored `certirocq:abi` of the shipped version to the shipped
/// cases. A different version is not checked here: it resolves fine, and the
/// gate rejects the `error` it defines by identity.
fn check_vendored_abi(resolve: &Resolve, wit: &Path) -> Result<()> {
    let shipped = abi_package_name();
    if !resolve.package_names.contains_key(shipped) {
        return Ok(());
    }
    let found = error_cases(resolve, shipped)
        .with_context(|| format!("reading the `{shipped}` vendored in {}", wit.display()))?;
    let expected = abi_error_cases();
    if found != expected {
        bail!(
            "the `{shipped}` in {} declares `{}.{}` as {:?}, but this generator \
             ships {:?}. An enum's case order is its canonical-ABI discriminant, \
             so these are different types under the same version -- either vendor \
             the shipped package unchanged or give yours its own version. See \
             TODO.md#abi-package-version",
            wit.display(),
            ABI_INTERFACE,
            ABI_ERROR,
            found,
            expected,
        );
    }
    Ok(())
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
            "this generator ships `{shipped}`; a world must `use {}.{{{}}}` \
             with that exact version -- see TODO.md#abi-package-version",
            abi_use_path(),
            ABI_ERROR
        ));
    }
    err
}
