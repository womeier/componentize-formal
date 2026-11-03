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
//!   something merely because the walk didn't recognise it; `TypeDefKind::Unknown`
//!   is itself a rejection.
//! - Every offending position is reported, not just the first, so one run tells
//!   you the whole story.
//! - Anything that narrows a value rather than rejecting it -- `u64` into a
//!   63-bit integer, a result type with nowhere to put an out-of-memory signal --
//!   produces a warning that names the position. `--deny-warnings` turns those
//!   into failures.
//! - A construct that cannot be represented is rejected even when it is merely
//!   declared and never used by an export, because it still appears in the
//!   component's type.

use std::fmt;

use wit_parser::{Function, FunctionKind, Resolve, Type, TypeDefKind, TypeId, WorldItem};

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
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::OutOfScope => write!(f, "out of scope"),
            Reason::Planned(phase) => write!(f, "not implemented yet (planned, phase {phase})"),
            Reason::Later => write!(f, "not implemented yet (wanted, not scheduled)"),
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
    pub note: &'static str,
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
        note: &'static str,
    ) {
        self.rejections.push(Rejection {
            code,
            what: what.into(),
            site: site.into(),
            reason,
            anchor,
            note,
        });
    }
}

/// Where in a signature a type sits. The result of an export gets one extra
/// affordance -- a top-level `option`/`result` there is the out-of-memory
/// channel, which Tier 0 handles -- so position has to travel with the walk.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pos {
    Param,
    Result,
}

pub fn check_world(resolve: &Resolve, world: wit_parser::WorldId) -> Report {
    let mut rep = Report::default();
    let w = &resolve.worlds[world];

    // `use` statements and world-level type definitions show up as imported
    // types; those are fine, they are just names. Imported *functions* and
    // *interfaces* are what would need mod_imports, which the correctness proof
    // fixes to [].
    for (key, item) in &w.imports {
        let name = resolve.name_world_key(key);
        let site = format!("world {}", w.name);
        match item {
            // A resource declared in a world desugars to an imported type plus
            // imported methods, so its methods arrive here. Report them as what
            // they are rather than as plain imports.
            WorldItem::Function(f) if f.kind != FunctionKind::Freestanding => rep.reject(
                "E-RESOURCE",
                format!("{name} (a resource method)"),
                site,
                Reason::OutOfScope,
                "resources",
                "resources need handle tables and destructors; there is no GC to hang them off",
            ),
            WorldItem::Function(_) | WorldItem::Interface { .. } => rep.reject(
                "E-IMPORT",
                format!("import {name}"),
                site,
                Reason::Later,
                "imports",
                "exports-only for now: mod_imports := [] is baked into the instantiation \
                 proof, so supporting imports means reworking it",
            ),
            WorldItem::Type { id, .. } => check_world_type(resolve, *id, &site, &mut rep),
        }
    }

    let mut exported_funcs = 0usize;
    for (key, item) in &w.exports {
        let name = resolve.name_world_key(key);
        match item {
            WorldItem::Function(f) => {
                exported_funcs += 1;
                check_function(resolve, f, &name, &mut rep);
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
            Reason::OutOfScope,
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

/// A type defined in or `use`d by the world. These carry no runtime surface of
/// their own -- the gate judges them where an export actually uses them -- with
/// one exception: a `resource` declaration belongs to the component's interface
/// whether or not an export mentions it, so it cannot be waved through.
fn check_world_type(resolve: &Resolve, id: TypeId, site: &str, rep: &mut Report) {
    let def = &resolve.types[id];
    if matches!(def.kind, TypeDefKind::Resource) {
        let shown = def.name.clone().unwrap_or_else(|| "resource".into());
        rep.reject(
            "E-RESOURCE",
            format!("resource {shown}"),
            site.to_string(),
            Reason::OutOfScope,
            "resources",
            "resources need handle tables and destructors; there is no GC to hang them off",
        );
    }
}

fn check_function(resolve: &Resolve, f: &Function, name: &str, rep: &mut Report) {
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
            "resources need handle tables and a destructor story; CertiRocq has neither",
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

    match f.result {
        Some(ty) => {
            walk(
                resolve,
                ty,
                Pos::Result,
                true,
                &format!("export {name}: result"),
                rep,
            );
            if !is_oom_channel(resolve, ty) {
                rep.warnings.push(format!(
                    "export {name}: result is not option<T> or result<T, E>, so the adapter has \
                     no way to report out-of-memory and will trap instead"
                ));
            }
        }
        None => rep.reject(
            "E-NO-RESULT",
            format!("func {name} returning nothing"),
            format!("export {name}"),
            Reason::Planned(2),
            "arity",
            "an export with no result has nothing to lift; it would still need the OOM \
             channel to be expressible",
        ),
    }
}

/// Does this result type give us somewhere to put an out-of-memory signal?
fn is_oom_channel(resolve: &Resolve, ty: Type) -> bool {
    match ty {
        Type::Id(id) => match &resolve.types[id].kind {
            TypeDefKind::Option(_) | TypeDefKind::Result(_) => true,
            TypeDefKind::Type(inner) => is_oom_channel(resolve, *inner),
            _ => false,
        },
        _ => false,
    }
}

/// Walk a type, rejecting anything we cannot lift or lower.
///
/// `top` marks the outermost type of a parameter or result. At the top of a
/// result, `option`/`result` is the out-of-memory channel rather than a
/// structured type, so we look through it; anywhere else it is a variant and
/// needs the full canonical-ABI layout work.
fn walk(resolve: &Resolve, ty: Type, pos: Pos, top: bool, site: &str, rep: &mut Report) {
    match ty {
        // Integers are carried by Rocq's primitive 63-bit ints, and u64 is the
        // only width wired up so far.
        Type::Bool => {}
        Type::U8 | Type::U16 | Type::U32 => rep.reject(
            "E-INT-WIDTH",
            unsigned_name(ty),
            site.to_string(),
            Reason::Later,
            "integer-widths",
            "u64 is the only width carried so far; a narrower one also needs a range check \
             on the way back out, because nothing stops the Rocq side returning a larger int",
        ),
        Type::U64 => rep.warnings.push(format!(
            "{site}: u64 is carried by Uint63.int, so values >= 2^63 are not representable; \
             the adapter guards instead of truncating"
        )),
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
            "waiting on the backend's wasm-prim-floats branch, where an f64 is boxed just \
             as a primitive int is -- a pointer to 8 bytes holding the value -- so the lift \
             and lower are the int63 shape with an f64 load and store",
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

fn walk_id(resolve: &Resolve, id: TypeId, pos: Pos, top: bool, site: &str, rep: &mut Report) {
    let def = &resolve.types[id];
    let shown = def.name.clone().unwrap_or_else(|| def.kind.as_str().into());

    match &def.kind {
        // A plain alias is transparent; keep `top` so `type answer = option<u64>`
        // still reads as the OOM channel.
        TypeDefKind::Type(inner) => walk(resolve, *inner, pos, top, site, rep),

        // All-nullary inductive: the tagged immediate 2*ord+1 on our side, a
        // discriminant on theirs. Cheapest structured type there is.
        TypeDefKind::Enum(_) => {}

        TypeDefKind::Option(inner) => {
            if top && pos == Pos::Result {
                walk(resolve, *inner, pos, false, site, rep);
            } else {
                rep.reject(
                    "E-OPTION",
                    format!("option<_> ({shown})"),
                    site.to_string(),
                    Reason::Planned(2),
                    "option-result",
                    "handled in the result position, where it carries the out-of-memory \
                     signal; as a parameter or nested it still needs lowering into the heap",
                );
            }
        }
        TypeDefKind::Result(r) => {
            if top && pos == Pos::Result {
                if let Some(ok) = r.ok {
                    walk(resolve, ok, pos, false, site, rep);
                }
                if let Some(err) = r.err {
                    walk(resolve, err, pos, false, site, rep);
                }
            } else {
                rep.reject(
                    "E-OPTION",
                    format!("result<_, _> ({shown})"),
                    site.to_string(),
                    Reason::Planned(2),
                    "option-result",
                    "handled in the result position, where it carries the out-of-memory \
                     signal; as a parameter or nested it still needs lowering into the heap",
                );
            }
        }

        TypeDefKind::Record(_) | TypeDefKind::Tuple(_) | TypeDefKind::Variant(_) => rep.reject(
            "E-STRUCTURED",
            format!("{} ({shown})", def.kind.as_str()),
            site.to_string(),
            Reason::Planned(3),
            "structured-types",
            "needs the split boxed/unboxed ordinal mapping and canonical-ABI record and \
             variant layout",
        ),

        TypeDefKind::Flags(_) => rep.reject(
            "E-FLAGS",
            format!("flags ({shown})"),
            site.to_string(),
            Reason::OutOfScope,
            "flags",
            "a bitfield has no natural Rocq counterpart; a record of bools would be a \
             different WIT type",
        ),

        TypeDefKind::List(_) | TypeDefKind::FixedLengthList(..) => rep.reject(
            "E-LIST",
            format!("{} ({shown})", def.kind.as_str()),
            site.to_string(),
            Reason::OutOfScope,
            "lists",
            "needs cabi_realloc and cons-cell walking in both directions, the one part that \
             requires real canonical-ABI machinery",
        ),

        TypeDefKind::Map(..) => rep.reject(
            "E-MAP",
            format!("map ({shown})"),
            site.to_string(),
            Reason::OutOfScope,
            "lists",
            "same machinery as lists, plus a key ordering the Rocq side would have to agree on",
        ),

        TypeDefKind::Resource | TypeDefKind::Handle(_) => rep.reject(
            "E-RESOURCE",
            format!("{} ({shown})", def.kind.as_str()),
            site.to_string(),
            Reason::OutOfScope,
            "resources",
            "resources need handle tables and destructors; there is no GC to hang them off",
        ),

        TypeDefKind::Future(_) | TypeDefKind::Stream(_) => rep.reject(
            "E-ASYNC",
            format!("{} ({shown})", def.kind.as_str()),
            site.to_string(),
            Reason::OutOfScope,
            "async",
            "the async ABI is a moving target and the backend is synchronous",
        ),

        TypeDefKind::Unknown => rep.reject(
            "E-UNKNOWN",
            format!("an unresolved type ({shown})"),
            site.to_string(),
            Reason::OutOfScope,
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
