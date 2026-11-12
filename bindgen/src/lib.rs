//! Generate a Rocq interface, a CertiRocq extraction file and canonical-ABI
//! glue from a WIT world.

pub mod codes;
pub mod config;
pub mod emit;
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
pub(crate) fn abi_error_cases() -> &'static [String] {
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

/// The case names of an error enum reached through however many aliases, in
/// declaration order.
///
/// The gate has already established that this is the shipped `error` by
/// identity, so this is how the emitters learn which discriminant each failure
/// gets: by name, from the type the component will actually declare, rather
/// than from a constant here that could disagree with it.
pub fn error_case_names(resolve: &Resolve, ty: wit_parser::Type) -> Result<Vec<String>> {
    let wit_parser::Type::Id(id) = ty else {
        bail!("the error half is a primitive type, not an enum")
    };
    match &resolve.types[id].kind {
        TypeDefKind::Type(inner) => error_case_names(resolve, *inner),
        TypeDefKind::Enum(e) => Ok(e.cases.iter().map(|c| c.name.clone()).collect()),
        kind => bail!("the error half is {}, not an enum", kind.as_str()),
    }
}

/// Load a WIT file or directory and pick the world to work on.
///
/// Two layouts have to work and they want opposite things. A bare world file
/// needs the shipped `certirocq:abi` pushed under it, so
/// `use certirocq:abi/errors@0.1.0.{error}` resolves with nothing vendored. A
/// directory laid out as a WIT package already carries the copy under
/// `deps/` --- it has to, since that copy is what any later resolve of the
/// directory reads --- and pushing ours as well is a duplicate package, which
/// `wit-parser` panics on rather than reporting.
///
/// Which one it is, is therefore decided by *looking* rather than by trying
/// and falling back. Trying first reads well until the world file has a typo
/// in it: the first attempt fails for that unrelated reason, the fallback
/// pushes our package on top of the vendored one, and a misplaced comma comes
/// out as a panic from inside `wit-parser`.
pub fn resolve_world(wit: &Path, world: Option<&str>) -> Result<(Resolve, WorldId)> {
    let vendored = target_declares_abi(wit)?;

    let mut resolve = Resolve::new();
    if !vendored {
        resolve
            .push_group(parse_abi()?)
            .context("loading the shipped certirocq:abi package")?;
    }
    let (pkg, _files) = resolve.push_path(wit).map_err(|e| abi_hint(e, wit))?;

    // Trusted for its name by the gate, so held to its contents here: the same
    // version with different cases would mean the adapter writing one
    // discriminant while the component declares another.
    match vendored {
        true => check_vendored_abi(&resolve, wit)?,
        // We supplied the package, but the directory may still carry its
        // own copy: resolving a *file* does not read the `deps/` beside it,
        // and anything resolving the directory does.
        false => check_abi_beside(wit)?,
    }

    let world = resolve
        .select_world(&[pkg], world)
        .context("selecting a world")?;
    Ok((resolve, world))
}

/// Does the path we are about to resolve supply `certirocq:abi` itself?
///
/// A directory brings its `deps/` with it. A single file does not --
/// `Resolve::push_path` on a file reads that file alone -- which is why a file
/// and the directory it sits in can resolve the same world against different
/// copies of the package, and why [`check_abi_beside`] exists.
fn target_declares_abi(wit: &Path) -> Result<bool> {
    if wit.is_dir() {
        return Ok(vendored_abi(wit)?.is_some());
    }
    // A world file could declare the package itself, which is unusual but
    // perfectly valid, and would collide just the same.
    let text =
        std::fs::read_to_string(wit).with_context(|| format!("reading {}", wit.display()))?;
    Ok(declares_abi(wit, &text))
}

/// The vendored copy of the shipped package in a WIT tree, as its path and
/// text.
///
/// Found by parsing each candidate's package declaration rather than by
/// resolving the tree: a tree that does not resolve must not be reported as
/// having no vendored package, because the answer decides whether ours gets
/// pushed on top of theirs.
fn vendored_abi(dir: &Path) -> Result<Option<(std::path::PathBuf, String)>> {
    let deps = dir.join("deps");
    if !deps.is_dir() {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(&deps).with_context(|| format!("reading {}", deps.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            for entry in
                std::fs::read_dir(&path).with_context(|| format!("reading {}", path.display()))?
            {
                candidates.push(entry?.path());
            }
        } else {
            candidates.push(path);
        }
    }
    for path in candidates {
        if path.extension().is_some_and(|e| e == "wit") {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            if declares_abi(&path, &text) {
                return Ok(Some((path, text)));
            }
        }
    }
    Ok(None)
}

/// Is this file the shipped package, name and version?
///
/// A file that does not parse is not it: the one that follows will report the
/// parse error against the user's own file, which is a better answer than
/// anything guessed from here.
fn declares_abi(path: &Path, text: &str) -> bool {
    UnresolvedPackageGroup::parse(path, text)
        .is_ok_and(|group| &group.main.name == abi_package_name())
}

/// Hold a vendored `certirocq:abi` of the shipped version to the shipped
/// cases. A different version is not checked here: it resolves fine, and the
/// gate rejects the `error` it defines by identity.
fn check_vendored_abi(resolve: &Resolve, wit: &Path) -> Result<()> {
    let shipped = abi_package_name();
    if !resolve.package_names.contains_key(shipped) {
        return Ok(());
    }
    compare_abi_cases(
        &error_cases(resolve, shipped)
            .with_context(|| format!("reading the `{shipped}` vendored in {}", wit.display()))?,
        wit,
    )
}

/// The same check for the copy on disk beside a world file.
///
/// Anything resolving the world from the directory -- this tool's next run,
/// `wasm-tools component wit` -- reads that copy, even when this run resolved
/// the world from one file. Without this, a vendored package whose cases had
/// been reordered would pass the gate today and redefine every discriminant
/// the next time anything resolves the directory.
fn check_abi_beside(wit: &Path) -> Result<()> {
    let dir = match wit.is_dir() {
        true => wit.to_path_buf(),
        false => match wit.parent().filter(|p| !p.as_os_str().is_empty()) {
            Some(parent) => parent.to_path_buf(),
            None => Path::new(".").to_path_buf(),
        },
    };
    let Some((path, text)) = vendored_abi(&dir)? else {
        return Ok(());
    };

    let mut resolve = Resolve::new();
    let group = UnresolvedPackageGroup::parse(&path, &text)
        .map_err(|(_, e)| e)
        .with_context(|| format!("parsing the `{}` in {}", abi_package_name(), path.display()))?;
    resolve.push_group(group).with_context(|| {
        format!(
            "the `{}` in {} does not resolve on its own; the shipped package \
             depends on nothing, so a copy that does is a different package. See \
             TODO.md#abi-package-version",
            abi_package_name(),
            path.display()
        )
    })?;
    compare_abi_cases(&error_cases(&resolve, abi_package_name())?, &path)
}

fn compare_abi_cases(found: &[String], where_: &Path) -> Result<()> {
    let expected = abi_error_cases();
    if found != expected {
        bail!(
            "the `{}` in {} declares `{}.{}` as {:?}, but this generator \
             ships {:?}. An enum's case order is its canonical-ABI discriminant, \
             so these are different types under the same version -- either vendor \
             the shipped package unchanged or give yours its own version. See \
             TODO.md#abi-package-version",
            abi_package_name(),
            where_.display(),
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
