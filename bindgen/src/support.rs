//! The support gate: decide whether a WIT world is something we can actually
//! compile against CertiRocq's Wasm backend, and say precisely why not when it
//! isn't.
//!
//! Every rejection carries an anchor into `TODO.md`, which is the single place
//! describing what is deferred and what it would take. Keep the two in sync: if
//! you add a code here, add the matching section there.
//!
//! # No silent failure
//!
//! A wrong adapter does not announce itself -- it reads the wrong bytes and
//! `wasmtime` hands back plausible garbage. So the rules here are deliberately
//! one-sided:
//!
//! - Every construct is matched explicitly. There is no catch-all that accepts
//!   something merely because the walk didn't recognise it;
//!   `TypeDefKind::Unknown` is itself a rejection.
//! - Every offending position is reported, not just the first, so one run tells
//!   you the whole story.
//! - Every export must be able to report every way it can fail. See
//!   [`channel`].
//! - A construct that cannot be represented is rejected even when it is merely
//!   declared and never used by an export, because it still appears in the
//!   component's type.

use std::fmt;

use wit_parser::{
    Function, FunctionKind, Resolve, Type, TypeDefKind, TypeId, TypeOwner, WorldItem,
};

use crate::config::Oom;

/// Why we are turning a world away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Deliberately outside the scope of this tool. `TODO.md` records what it
    /// would cost if that ever changes.
    OutOfScope,
    /// Part of the phased plan, just not built yet.
    Planned(u8),
    /// Wanted eventually, but not scheduled -- nothing depends on it today.
    Later,
    /// Not a deferred feature: the world as written cannot be compiled at all.
    Malformed,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::OutOfScope => write!(f, "out of scope"),
            Reason::Planned(phase) => write!(f, "not implemented yet (planned, phase {phase})"),
            Reason::Later => write!(f, "not implemented yet (wanted, not scheduled)"),
            Reason::Malformed => write!(f, "cannot be compiled"),
        }
    }
}

pub struct Rejection {
    /// Stable identifier, also the `TODO.md` anchor suffix.
    pub code: &'static str,
    /// The offending construct, as it reads in the WIT file.
    pub what: String,
    /// Where it was found, in terms a reader of the WIT can locate.
    pub site: String,
    pub reason: Reason,
    pub anchor: &'static str,
    pub note: String,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "error[{}]: {} is not supported", self.code, self.what)?;
        writeln!(f, "  --> {}", self.site)?;
        writeln!(f, "  = {}: {}", self.reason, self.note)?;
        write!(f, "  = see TODO.md#{}", self.anchor)
    }
}

#[derive(Default)]
pub struct Report {
    pub rejections: Vec<Rejection>,
    pub warnings: Vec<String>,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        self.rejections.is_empty()
    }

    fn reject(
        &mut self,
        code: &'static str,
        what: impl Into<String>,
        site: impl Into<String>,
        reason: Reason,
        anchor: &'static str,
        note: impl Into<String>,
    ) {
        self.rejections.push(Rejection {
            code,
            what: what.into(),
            site: site.into(),
            reason,
            anchor,
            note: note.into(),
        });
    }
}

/// A way a call can fail that the caller has to be told about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// `glob_out_of_mem` was set. Possible on every call, because
    /// `main_function` runs the whole program and the adapter allocates the
    /// area it returns through.
    OutOfMemory,
    /// A value crossing the boundary has no representation on the other side.
    OutOfRange(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::OutOfMemory => write!(f, "out of memory"),
            Failure::OutOfRange(what) => write!(f, "out of range ({what})"),
        }
    }
}

/// Where in a signature a type sits. Lowering and lifting fail in different
/// directions, so position travels with the walk.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pos {
    Param,
    Result,
}

pub fn check_world(resolve: &Resolve, world: wit_parser::WorldId, oom: Oom) -> Report {
    let mut rep = Report::default();
    let w = &resolve.worlds[world];

    for (key, item) in &w.imports {
        let name = resolve.name_world_key(key);
        let site = format!("world {}", w.name);
        match item {
            // A resource declared in a world desugars to an imported type plus
            // imported methods, so its methods arrive here. Report them as
            // what they are rather than as plain imports.
            WorldItem::Function(f) if f.kind != FunctionKind::Freestanding => rep.reject(
                "E-RESOURCE",
                format!("{name} (a resource method)"),
                site,
                Reason::OutOfScope,
                "resources",
                RESOURCE_NOTE,
            ),
            WorldItem::Function(_) => rep.reject(
                "E-IMPORT",
                format!("import {name}"),
                site,
                Reason::Later,
                "imports",
                IMPORT_NOTE,
            ),
            // `use certirocq:abi/errors.{error}` pulls the interface in here.
            // A type-only interface contributes nothing to the core module's
            // imports, so it costs nothing; one with functions is a real
            // import.
            WorldItem::Interface { id, .. } => {
                if resolve.interfaces[*id].functions.is_empty() {
                    for (_, ty) in &resolve.interfaces[*id].types {
                        check_world_type(resolve, *ty, &site, &mut rep);
                    }
                } else {
                    rep.reject(
                        "E-IMPORT",
                        format!("import {name}"),
                        site,
                        Reason::Later,
                        "imports",
                        IMPORT_NOTE,
                    );
                }
            }
            WorldItem::Type { id, .. } => check_world_type(resolve, *id, &site, &mut rep),
        }
    }

    let mut exported_funcs = 0usize;
    for (key, item) in &w.exports {
        let name = resolve.name_world_key(key);
        match item {
            WorldItem::Function(f) => {
                exported_funcs += 1;
                check_function(resolve, f, &name, oom, &mut rep);
            }
            WorldItem::Interface { .. } => rep.reject(
                "E-EXPORT-INTERFACE",
                format!("export {name}"),
                format!("world {}", w.name),
                Reason::OutOfScope,
                "exported-interfaces",
                "only top-level `export name: func(..)` is handled; a namespaced interface \
                 export needs its own naming scheme in the adapter",
            ),
            WorldItem::Type { id, .. } => {
                check_world_type(resolve, *id, &format!("world {}", w.name), &mut rep)
            }
        }
    }

    if exported_funcs == 0 {
        rep.reject(
            "E-NO-EXPORT",
            "a world with no exported functions",
            format!("world {}", w.name),
            Reason::Malformed,
            "nothing-to-compile",
            "there would be nothing for CertiRocq to compile",
        );
    }
    if exported_funcs > 1 {
        rep.reject(
            "E-MULTI-EXPORT",
            format!("{exported_funcs} exported functions"),
            format!("world {}", w.name),
            Reason::Planned(2),
            "multi-export",
            "needs the generated MkExports constructor so one compiled term carries every \
             export closure",
        );
    }

    rep
}

const IMPORT_NOTE: &str = "exports-only for now: mod_imports := [] is baked into the \
                           instantiation proof, so supporting imports means reworking it";
const RESOURCE_NOTE: &str = "resources need handle tables and destructors; there is no GC to \
                             hang them off";

/// A type defined in or `use`d by the world. These carry no runtime surface of
/// their own -- the gate judges them where an export actually uses them -- with
/// one exception: a `resource` declaration belongs to the component's interface
/// whether or not an export mentions it, so it cannot be waved through.
fn check_world_type(resolve: &Resolve, id: TypeId, site: &str, rep: &mut Report) {
    let def = &resolve.types[id];
    if matches!(def.kind, TypeDefKind::Resource) {
        rep.reject(
            "E-RESOURCE",
            describe(def),
            site.to_string(),
            Reason::OutOfScope,
            "resources",
            RESOURCE_NOTE,
        );
    }
}

fn check_function(resolve: &Resolve, f: &Function, name: &str, oom: Oom, rep: &mut Report) {
    match f.kind {
        FunctionKind::Freestanding => {}
        FunctionKind::AsyncFreestanding => rep.reject(
            "E-ASYNC",
            format!("async func {name}"),
            format!("export {name}"),
            Reason::OutOfScope,
            "async",
            "the async ABI, futures and streams are a moving target and the backend is \
             single-threaded and synchronous",
        ),
        _ => rep.reject(
            "E-RESOURCE",
            format!("func {name} (a resource method)"),
            format!("export {name}"),
            Reason::OutOfScope,
            "resources",
            RESOURCE_NOTE,
        ),
    }

    match f.params.len() {
        1 => {}
        0 => rep.reject(
            "E-ARITY-0",
            format!("func {name}() with no parameters"),
            format!("export {name}"),
            Reason::Planned(2),
            "arity",
            "a nullary export is a value, not a closure, so the adapter reads glob_result \
             directly instead of calling through the table",
        ),
        n => rep.reject(
            "E-ARITY-N",
            format!("func {name} with {n} parameters"),
            format!("export {name}"),
            Reason::Planned(2),
            "arity",
            "needs iterated closure application, one argument per call_indirect, as the C \
             glue's make_call does",
        ),
    }

    for p in &f.params {
        walk(
            resolve,
            p.ty,
            Pos::Param,
            true,
            &format!("export {name}: param `{}`", p.name),
            rep,
        );
    }

    let Some(result) = f.result else {
        rep.reject(
            "E-NO-RESULT",
            format!("func {name} returning nothing"),
            format!("export {name}"),
            Reason::Planned(2),
            "arity",
            "an export with no result has nothing to lift, and nowhere to report failure",
        );
        return;
    };

    let failures = failure_modes(resolve, f);

    match channel(resolve, result) {
        Channel::AbiError(ok) => {
            // The error half is ours; only the ok half is the Rocq function's.
            if let Some(ok) = ok {
                walk(
                    resolve,
                    ok,
                    Pos::Result,
                    true,
                    &format!("export {name}: result"),
                    rep,
                );
            }
        }
        other => {
            // The type in this position is the channel, however badly it
            // carries failures, so report that once rather than also calling
            // it a structured type. Its payloads still get walked.
            let site = format!("export {name}: result");
            match payloads(resolve, result) {
                Some(payloads) => {
                    for ty in payloads {
                        walk(resolve, ty, Pos::Result, false, &site, rep);
                    }
                }
                None => walk(resolve, result, Pos::Result, true, &site, rep),
            }
            if oom == Oom::Trap {
                for failure in &failures {
                    rep.warnings.push(format!(
                        "export {name}: {failure} will trap, because the result type carries \
                         no error channel"
                    ));
                }
            } else {
                let listed = failures
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                match other {
                    Channel::WrongError(shown) => rep.reject(
                        "E-ERROR-TYPE",
                        format!("result<_, {shown}>"),
                        format!("export {name}: result"),
                        Reason::Malformed,
                        "failure-channel",
                        format!(
                            "the error half must be certirocq:abi/errors.error, which is the \
                             only type the adapter knows how to populate; this call can fail \
                             with: {listed}"
                        ),
                    ),
                    _ => rep.reject(
                        "E-NO-CHANNEL",
                        "a result type with no error channel",
                        format!("export {name}: result"),
                        Reason::Malformed,
                        "failure-channel",
                        format!(
                            "every export must return result<T, error> with \
                             certirocq:abi/errors.error, because this call can fail with: \
                             {listed}. Pass --oom=trap to trap instead."
                        ),
                    ),
                }
            }
        }
    }
}

/// The payloads of an `option` or `result`, or `None` if this is neither.
fn payloads(resolve: &Resolve, ty: Type) -> Option<Vec<Type>> {
    let Type::Id(id) = ty else {
        return None;
    };
    match &resolve.types[id].kind {
        TypeDefKind::Type(inner) => payloads(resolve, *inner),
        TypeDefKind::Option(inner) => Some(vec![*inner]),
        TypeDefKind::Result(r) => Some(r.ok.into_iter().chain(r.err).collect()),
        _ => None,
    }
}

/// What the declared result type can report through.
enum Channel {
    /// `result<T, certirocq:abi/errors.error>`, carrying the ok type.
    AbiError(Option<Type>),
    /// A `result<_, E>` whose error half is something else.
    WrongError(String),
    /// Anything else: a bare type, an `option`, a `result` without an error.
    None,
}

fn channel(resolve: &Resolve, ty: Type) -> Channel {
    let Type::Id(id) = ty else {
        return Channel::None;
    };
    match &resolve.types[id].kind {
        TypeDefKind::Type(inner) => channel(resolve, *inner),
        TypeDefKind::Result(r) => match r.err {
            Some(err) if is_abi_error(resolve, err) => Channel::AbiError(r.ok),
            Some(err) => Channel::WrongError(show(resolve, err)),
            None => Channel::None,
        },
        _ => Channel::None,
    }
}

/// Is this the `error` enum from the shipped `certirocq:abi` package?
///
/// Checked by package, interface and type name rather than by case names, so a
/// look-alike enum of the user's own is rejected instead of being populated
/// with cases that happen to line up.
fn is_abi_error(resolve: &Resolve, ty: Type) -> bool {
    let Type::Id(id) = ty else {
        return false;
    };
    let def = &resolve.types[id];
    // `use certirocq:abi/errors.{error}` introduces an alias owned by the
    // world, so follow it to the interface that actually defines the enum.
    if let TypeDefKind::Type(inner) = def.kind {
        return is_abi_error(resolve, inner);
    }
    if def.name.as_deref() != Some(crate::ABI_ERROR) {
        return false;
    }
    let TypeOwner::Interface(iface_id) = def.owner else {
        return false;
    };
    let iface = &resolve.interfaces[iface_id];
    if iface.name.as_deref() != Some(crate::ABI_INTERFACE) {
        return false;
    }
    iface.package.is_some_and(|pkg| {
        let name = &resolve.packages[pkg].name;
        (name.namespace.as_str(), name.name.as_str()) == crate::ABI_PACKAGE
    })
}

/// Every way a call to this export can fail.
///
/// Out of memory is unconditional. The rest come from values that have no
/// representation on the other side -- today only a `u64` argument of 2^63 or
/// more, since lifting a 63-bit int into a `u64` is always exact and the
/// narrower widths are rejected outright.
pub fn failure_modes(resolve: &Resolve, f: &Function) -> Vec<Failure> {
    let mut failures = vec![Failure::OutOfMemory];
    for p in &f.params {
        if matches!(resolved(resolve, p.ty), Type::U64) {
            failures.push(Failure::OutOfRange(format!("`{}` >= 2^63", p.name)));
        }
    }
    failures
}

/// Look through type aliases.
fn resolved(resolve: &Resolve, ty: Type) -> Type {
    match ty {
        Type::Id(id) => match &resolve.types[id].kind {
            TypeDefKind::Type(inner) => resolved(resolve, *inner),
            _ => ty,
        },
        _ => ty,
    }
}

fn show(resolve: &Resolve, ty: Type) -> String {
    match ty {
        Type::Id(id) => resolve.types[id]
            .name
            .clone()
            .unwrap_or_else(|| resolve.types[id].kind.as_str().to_string()),
        other => format!("{other:?}").to_lowercase(),
    }
}

/// Walk a type, rejecting anything we cannot lift or lower.
fn walk(resolve: &Resolve, ty: Type, pos: Pos, top: bool, site: &str, rep: &mut Report) {
    match ty {
        // Integers are carried by Rocq's primitive 63-bit ints, and u64 is the
        // only width wired up so far.
        Type::Bool => {}
        // Lowering a u64 can overflow Uint63.int; lifting a 63-bit int into a
        // u64 never can. The overflow is not warned about here because it is
        // already accounted for as a failure mode -- see `failure_modes` --
        // and reported through the error channel.
        Type::U64 => {}
        Type::U8 | Type::U16 | Type::U32 => rep.reject(
            "E-INT-WIDTH",
            unsigned_name(ty),
            site.to_string(),
            Reason::Later,
            "integer-widths",
            "u64 is the only width carried so far; a narrower one also needs a range check \
             on the way back out, because nothing stops the Rocq side returning a larger int",
        ),
        Type::S8 | Type::S16 | Type::S32 | Type::S64 => rep.reject(
            "E-SIGNED",
            signed_name(ty),
            site.to_string(),
            Reason::Later,
            "signed-integers",
            "only unsigned integers are carried, via Rocq's primitive 63-bit ints; signed \
             ones need a choice between Sint63 and a two's-complement reading of Uint63",
        ),
        Type::F64 => rep.reject(
            "E-FLOAT",
            "f64",
            site.to_string(),
            Reason::Planned(4),
            "f64",
            "waiting on the backend's wasm-prim-floats branch, where an f64 is boxed just as \
             a primitive int is -- a pointer to 8 bytes holding the value -- so the lift and \
             lower are the int63 shape with an f64 load and store",
        ),
        Type::F32 => rep.reject(
            "E-FLOAT",
            "f32",
            site.to_string(),
            Reason::OutOfScope,
            "f32",
            "Rocq's PrimFloat is binary64 only, so there is nothing to carry an f32 \
             faithfully; f64 is the supported width",
        ),
        Type::Char => rep.reject(
            "E-CHAR",
            "char",
            site.to_string(),
            Reason::OutOfScope,
            "char",
            "a Unicode scalar value has no direct counterpart; Rocq's ascii is 8-bit and \
             Stdlib has no char type matching WIT's",
        ),
        Type::String => rep.reject(
            "E-STRING",
            "string",
            site.to_string(),
            Reason::Later,
            "strings",
            "wanted, but a string is bytes under an encoding choice, so it brings \
             cabi_realloc and walking the Rocq value in both directions with it",
        ),
        Type::ErrorContext => rep.reject(
            "E-ASYNC",
            "error-context",
            site.to_string(),
            Reason::OutOfScope,
            "async",
            "part of the async ABI",
        ),
        Type::Id(id) => walk_id(resolve, id, pos, top, site, rep),
    }
}

/// How a type reads in a diagnostic: its kind, plus its name when it has one.
/// An anonymous `list<u64>` is just "list"; a declared record is
/// "record (point)".
fn describe(def: &wit_parser::TypeDef) -> String {
    match &def.name {
        Some(name) => format!("{} ({name})", def.kind.as_str()),
        None => def.kind.as_str().to_string(),
    }
}

fn walk_id(resolve: &Resolve, id: TypeId, pos: Pos, top: bool, site: &str, rep: &mut Report) {
    let def = &resolve.types[id];
    let shown = describe(def);

    match &def.kind {
        TypeDefKind::Type(inner) => walk(resolve, *inner, pos, top, site, rep),

        // All-nullary inductive: the tagged immediate 2*ord+1 on our side, a
        // discriminant on theirs. Cheapest structured type there is.
        TypeDefKind::Enum(_) => {}

        // The error channel is handled before the walk gets here, so an
        // option or result at this point is an ordinary structured type.
        TypeDefKind::Option(_) | TypeDefKind::Result(_) => rep.reject(
            "E-OPTION",
            shown.clone(),
            site.to_string(),
            Reason::Planned(2),
            "option-result",
            "needs lowering into, or lifting out of, the heap; cheaper than the general \
             structured types because the shapes are fixed and the ordinals known",
        ),

        TypeDefKind::Record(_) | TypeDefKind::Tuple(_) | TypeDefKind::Variant(_) => rep.reject(
            "E-STRUCTURED",
            shown.clone(),
            site.to_string(),
            Reason::Planned(3),
            "structured-types",
            "needs the split boxed/unboxed ordinal mapping and canonical-ABI record and \
             variant layout",
        ),

        TypeDefKind::Flags(_) => rep.reject(
            "E-FLAGS",
            shown.clone(),
            site.to_string(),
            Reason::OutOfScope,
            "flags",
            "a bitfield has no natural Rocq counterpart; a record of bools would be a \
             different WIT type",
        ),

        TypeDefKind::List(_) | TypeDefKind::FixedLengthList(..) => rep.reject(
            "E-LIST",
            shown.clone(),
            site.to_string(),
            Reason::Later,
            "lists",
            "wanted: list is the one recursive former the component model has, so it is \
             expressible -- but it needs cabi_realloc and cons-cell walking in both \
             directions",
        ),

        TypeDefKind::Map(..) => rep.reject(
            "E-MAP",
            shown.clone(),
            site.to_string(),
            Reason::OutOfScope,
            "maps",
            "the list machinery plus a key ordering both sides would have to agree on, with \
             no canonical Rocq map type to agree with",
        ),

        TypeDefKind::Resource | TypeDefKind::Handle(_) => rep.reject(
            "E-RESOURCE",
            shown.clone(),
            site.to_string(),
            Reason::OutOfScope,
            "resources",
            RESOURCE_NOTE,
        ),

        TypeDefKind::Future(_) | TypeDefKind::Stream(_) => rep.reject(
            "E-ASYNC",
            shown.clone(),
            site.to_string(),
            Reason::OutOfScope,
            "async",
            "the async ABI is a moving target and the backend is synchronous",
        ),

        TypeDefKind::Unknown => rep.reject(
            "E-UNKNOWN",
            format!("an unresolved type: {shown}"),
            site.to_string(),
            Reason::Malformed,
            "unresolved",
            "the world did not resolve; this usually means a missing dependency package",
        ),
    }
}

fn unsigned_name(ty: Type) -> &'static str {
    match ty {
        Type::U8 => "u8",
        Type::U16 => "u16",
        _ => "u32",
    }
}

fn signed_name(ty: Type) -> &'static str {
    match ty {
        Type::S8 => "s8",
        Type::S16 => "s16",
        Type::S32 => "s32",
        _ => "s64",
    }
}
