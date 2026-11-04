//! Everything the emitters need, worked out once and checked here.
//!
//! The emitters below this module cannot fail: they format a `Plan` into text.
//! That is deliberate. Every question with a wrong answer -- which Rocq type
//! carries a WIT type, where the payload sits in the return area, whether the
//! argument needs boxing, which failures the adapter has to be able to report
//! -- is answered once, here, and either answered or refused. An emitter that
//! could fail halfway would leave a directory of files that look generated and
//! do not agree with each other.

use anyhow::{bail, ensure, Context, Result};
use wit_parser::{
    abi::{AbiVariant, WasmType},
    Function, Int, Resolve, SizeAlign, Type, TypeDefKind, WorldId, WorldItem,
};

use crate::config::Config;
use crate::repr;
use crate::support::{self, Failure};

/// How a WIT type is carried on the Rocq side, which decides both the
/// signature in `interface.v` and the lift and lower in the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repr {
    /// Rocq's `bool`. A nullary constructor, so an immediate rather than a
    /// pointer -- and the one inductive whose ordinals are not declaration
    /// order (`repr::BOOL_FALSE`).
    Bool,
    /// Rocq's primitive 63-bit int. Boxed: a pointer to an 8-byte payload.
    Int63,
    /// A generated all-nullary inductive. An immediate, like `Bool`, but in
    /// declaration order.
    Enum {
        /// The Rocq inductive's name.
        rocq_type: String,
        /// Case count, which bounds the discriminant.
        cases: usize,
        /// Width of the discriminant in the canonical ABI.
        tag: Int,
    },
}

impl Repr {
    /// The Rocq type as it is written in a signature.
    pub fn rocq_type(&self) -> &str {
        match self {
            Repr::Bool => "bool",
            Repr::Int63 => "int",
            Repr::Enum { rocq_type, .. } => rocq_type,
        }
    }

    /// Is the value a pointer into the heap rather than an immediate?
    ///
    /// This is the single fact the adapter's lift and lower turn on: an
    /// immediate is passed and returned as itself, a boxed value as the
    /// address of its payload.
    pub fn boxed(&self) -> bool {
        matches!(self, Repr::Int63)
    }

    /// WIT discriminant *i* corresponds to which CertiRocq immediate?
    ///
    /// For an `enum` the generator writes the inductive, so the ordinal is the
    /// declaration index by construction and this is `2*i+1`. For `bool` it is
    /// also `2*i+1`, but for a different reason worth stating: the pipeline
    /// swaps `bool`'s constructors, and the swap happens to make the ordinals
    /// agree with the component model's `false = 0, true = 1`. Two reasons, one
    /// table -- so the table is what the adapter is emitted from, and
    /// [`Self::tags_are_the_formula`] is what lets it be emitted as a shift.
    pub fn tags(&self) -> Vec<u32> {
        match self {
            Repr::Bool => vec![
                repr::BOOL_FALSE, // false is discriminant 0
                repr::BOOL_TRUE,  // true is discriminant 1
            ],
            Repr::Enum { cases, .. } => (0..*cases as u32).map(repr::nullary_immediate).collect(),
            Repr::Int63 => Vec::new(),
        }
    }

    /// Does the table above reduce to `2*d+1` for every case?
    ///
    /// When it does -- which is every type carried today -- the adapter lifts
    /// and lowers with a shift. When it ever does not, the shift would quietly
    /// map the user's cases onto the wrong constructors, so the emitter must
    /// refuse rather than assume.
    pub fn tags_are_the_formula(&self) -> bool {
        self.tags()
            .iter()
            .enumerate()
            .all(|(d, &tag)| tag == repr::nullary_immediate(d as u32))
    }
}

/// An inductive `interface.v` has to declare, because the world declared a
/// type with no Rocq counterpart to reuse.
#[derive(Debug, Clone)]
pub struct Inductive {
    pub rocq_name: String,
    /// Constructors in declaration order, which *is* the ordinal order and the
    /// canonical-ABI discriminant order. Emitting them in any other order
    /// would silently permute the user's cases.
    pub constructors: Vec<String>,
    /// The WIT name, for the comment that ties the two together.
    pub wit_name: String,
    /// The WIT case names, in the same order as `constructors`.
    pub wit_cases: Vec<String>,
}

impl Inductive {
    /// How the type reads in the WIT file, for the comment above the emitted
    /// inductive. Rebuilt rather than quoted, so it says what was understood.
    pub fn wit_declaration(&self) -> String {
        format!("enum {} {{ {} }}", self.wit_name, self.wit_cases.join(", "))
    }
}

/// Where things sit in the return area, in bytes.
#[derive(Debug, Clone)]
pub struct Layout {
    /// `result<T, error>`: size, alignment, and the offset its payload sits at.
    /// Computed with `wit_parser::SizeAlign`, which is the same code
    /// `wasm-tools component new` uses -- so these cannot disagree with the
    /// component the build assembles.
    pub ret_size: u32,
    pub ret_align: u32,
    pub payload: u32,
    /// Width of the `result` discriminant, and of the `error` enum's.
    pub ret_tag: Int,
    pub err_tag: Int,
    /// Bytes taken off `mem_ptr`: the return area, then the boxed argument.
    pub reserve: u32,
    /// Offset of the boxed argument from the start of the reservation, when
    /// the parameter is boxed.
    pub arg: Option<u32>,
}

/// The discriminants the adapter writes. All read off the types rather than
/// chosen, because an enum's case order *is* its discriminant.
#[derive(Debug, Clone)]
pub struct Discriminants {
    pub ok: u32,
    pub err: u32,
    pub out_of_memory: u32,
    /// `None` when no parameter can be out of range, in which case the adapter
    /// emits no range check either.
    pub out_of_range: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub wit_name: String,
    pub rocq_name: String,
    pub repr: Repr,
    /// The core wasm type the canonical ABI flattens this parameter to.
    pub wasm: &'static str,
}

#[derive(Debug, Clone)]
pub struct Export {
    /// The name in the WIT file, which is the component's export name.
    pub wit_name: String,
    /// The same name as a Rocq identifier.
    pub rocq_name: String,
    /// How the export reads in the world, for the comment at the top of each
    /// generated file.
    pub signature: String,
    pub param: Param,
    pub ok: Repr,
    pub layout: Layout,
    pub discriminants: Discriminants,
    pub failures: Vec<Failure>,
}

/// File names the generated build rules tie together.
#[derive(Debug, Clone)]
pub struct Build {
    /// `-file` for `CertiRocq Compile Wasm`, so the backend writes
    /// `<wasm_name>.wasm`.
    pub wasm_name: String,
    /// Rocq library names, which are the file stems.
    pub interface_stem: String,
    pub impl_stem: String,
    pub extraction_stem: String,
    /// The module inside `impl.v` to compile.
    pub impl_module: String,
    /// The WIT directory `wasm-tools component embed` reads, relative to the
    /// output directory.
    pub wit_dir: String,
    /// Passed through verbatim to `CertiRocq Compile Wasm`.
    pub flags: Vec<String>,
}

#[derive(Debug)]
pub struct Plan {
    /// The `Module Type`'s name, from the world's.
    pub module_type: String,
    /// The world as the WIT file spells it.
    pub world_name: String,
    pub export: Export,
    pub inductives: Vec<Inductive>,
    pub build: Build,
}

/// Names the generated Rocq files already use, so an export or a case called
/// one of these would shadow the thing the signature next to it refers to.
/// Rocq would accept some of them and mean something else.
const RESERVED: &[&str] = &[
    // What the generated signatures are written in terms of.
    "bool",
    "true",
    "false",
    "int",
    "unit",
    "nat",
    "option",
    "list",
    "Some",
    "None",
    // Rocq keywords and command heads that cannot be identifiers at all.
    "fun",
    "forall",
    "match",
    "with",
    "end",
    "in",
    "let",
    "if",
    "then",
    "else",
    "fix",
    "cofix",
    "Prop",
    "Set",
    "Type",
    "as",
    "return",
    "at",
    "where",
    "for",
    "of",
    "struct",
    "using",
    "exists",
    "exists2",
    "Definition",
    "Parameter",
    "Inductive",
    "Module",
    "End",
    "Require",
    "Import",
    "From",
];

impl Plan {
    /// Distil a checked world into the plan the emitters format.
    ///
    /// The gate has already run; this re-states the few of its conclusions the
    /// emitters depend on as assertions rather than trusting them, because the
    /// two can be changed independently and a gate that accepts more than the
    /// emitters handle is the shape of a silent failure.
    pub fn new(resolve: &Resolve, world: WorldId, cfg: &Config, wit_dir: &str) -> Result<Plan> {
        let w = &resolve.worlds[world];

        let mut funcs = w.exports.iter().filter_map(|(key, item)| match item {
            WorldItem::Function(f) => Some((resolve.name_world_key(key), f)),
            _ => None,
        });
        let (wit_name, func) = funcs
            .next()
            .context("the world exports no function; nothing to generate")?;
        ensure!(
            funcs.next().is_none(),
            "the world exports more than one function, which the gate should have \
             rejected (E-MULTI-EXPORT) -- see TODO.md#multi-export"
        );

        let mut inductives = Vec::new();
        let mut taken: Vec<String> = Vec::new();

        let rocq_name = rocq_ident(&wit_name, "export", &mut taken)?;

        ensure!(
            func.params.len() == 1,
            "`{wit_name}` takes {} parameters; only one is handled, which the gate \
             should have rejected (E-ARITY-0/E-ARITY-N) -- see TODO.md#arity",
            func.params.len()
        );
        let wit_param = &func.params[0];
        let param_repr = repr_of(resolve, wit_param.ty, &mut inductives, &mut taken)
            .with_context(|| format!("parameter `{}` of `{wit_name}`", wit_param.name))?;

        let result = func.result.context(
            "the export returns nothing, which the gate should have rejected \
                      (E-NO-RESULT) -- see TODO.md#arity",
        )?;
        let (ok, err) = result_halves(resolve, result).with_context(|| {
            format!(
                "the result of `{wit_name}` is not result<T, error>, which the gate \
                     should have rejected (E-NO-CHANNEL) -- see TODO.md#failure-channel"
            )
        })?;
        let ok = ok.context(
            "the export returns result<_, error> with no ok type, so there is nothing for \
             the Rocq function to return -- give the ok half a type",
        )?;
        let ok_repr = repr_of(resolve, ok, &mut inductives, &mut taken)
            .with_context(|| format!("the result of `{wit_name}`"))?;

        let sig = resolve.wasm_signature(AbiVariant::GuestExport, func);
        let layout = layout(resolve, result, &sig, &param_repr, ok, err)?;
        let discriminants = discriminants(resolve, err, &param_repr)?;

        // What the gate says can go wrong has to be what the adapter can
        // report. A failure mode added there without a branch here would
        // otherwise become a trap, or worse, a plausible answer.
        let failures = support::failure_modes(resolve, func);
        check_failures(&failures, &param_repr, &discriminants)?;

        // The parameter's name is also the wat local the adapter reads it
        // from, so it has to clear the locals the adapter declares itself --
        // two `$ret`s in one function is not a subtle failure, but it is one
        // the generator should never emit.
        let mut locals = super::adapter::LOCALS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let param = Param {
            wit_name: wit_param.name.clone(),
            rocq_name: rocq_ident(&wit_param.name, "parameter", &mut locals)?,
            wasm: wasm_type(&sig.params, &wit_name)?,
            repr: param_repr,
        };

        Ok(Plan {
            module_type: module_type_name(&w.name)?,
            world_name: w.name.clone(),
            export: Export {
                signature: signature(resolve, &wit_name, func),
                wit_name,
                rocq_name,
                param,
                ok: ok_repr,
                layout,
                discriminants,
                failures,
            },
            inductives,
            build: build(cfg, wit_dir)?,
        })
    }
}

/// `wrapper-world` names the `Module Type` `WrapperWorld`.
fn module_type_name(world: &str) -> Result<String> {
    let name: String = world
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    ensure!(
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "world `{world}` does not give a usable Rocq module name (`{name}`)"
    );
    Ok(name)
}

/// A WIT name as a Rocq identifier: kebab-case becomes snake_case.
///
/// Refuses rather than mangles. A name that needs more than this is rare, and
/// a generator that invents `plus1_` to dodge a collision produces a signature
/// the user did not write and cannot guess.
fn rocq_ident(wit: &str, what: &str, taken: &mut Vec<String>) -> Result<String> {
    let ident = wit.replace('-', "_");
    ensure!(
        !ident.is_empty()
            && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !ident.starts_with(|c: char| c.is_ascii_digit()),
        "{what} `{wit}` does not become a Rocq identifier (`{ident}`); rename it in the \
         WIT file"
    );
    ensure!(
        !RESERVED.contains(&ident.as_str()),
        "{what} `{wit}` becomes `{ident}`, which the generated Rocq file already uses \
         for something else; rename it in the WIT file"
    );
    ensure!(
        !taken.contains(&ident),
        "{what} `{wit}` becomes `{ident}`, which is already taken -- by another name \
         in this world, or by a local the generated adapter declares. Rename it in \
         the WIT file."
    );
    taken.push(ident.clone());
    Ok(ident)
}

/// Which Rocq type carries this WIT type, declaring an inductive if there is
/// no counterpart to reuse.
fn repr_of(
    resolve: &Resolve,
    ty: Type,
    inductives: &mut Vec<Inductive>,
    taken: &mut Vec<String>,
) -> Result<Repr> {
    match ty {
        Type::Bool => Ok(Repr::Bool),
        Type::U64 => Ok(Repr::Int63),
        Type::Id(id) => {
            let def = &resolve.types[id];
            match &def.kind {
                TypeDefKind::Type(inner) => repr_of(resolve, *inner, inductives, taken),
                TypeDefKind::Enum(e) => {
                    let wit_name = def
                        .name
                        .clone()
                        .context("an anonymous enum has no name to give the inductive")?;
                    let rocq_name = match inductives.iter().find(|i| i.wit_name == wit_name) {
                        // Used twice in the same signature: declare it once.
                        Some(existing) => existing.rocq_name.clone(),
                        None => {
                            let rocq_name = rocq_ident(&wit_name, "type", taken)?;
                            let constructors = e
                                .cases
                                .iter()
                                .map(|c| rocq_ident(&c.name, "enum case", taken))
                                .collect::<Result<Vec<_>>>()?;
                            inductives.push(Inductive {
                                rocq_name: rocq_name.clone(),
                                constructors,
                                wit_name,
                                wit_cases: e.cases.iter().map(|c| c.name.clone()).collect(),
                            });
                            rocq_name
                        }
                    };
                    Ok(Repr::Enum {
                        rocq_type: rocq_name,
                        cases: e.cases.len(),
                        tag: e.tag(),
                    })
                }
                kind => bail!(
                    "{} has no Rocq counterpart here, which the gate should have rejected \
                     -- see TODO.md",
                    kind.as_str()
                ),
            }
        }
        other => {
            bail!("{other:?} is not carried, which the gate should have rejected -- see TODO.md")
        }
    }
}

/// The ok and err halves of the declared result type.
fn result_halves(resolve: &Resolve, ty: Type) -> Result<(Option<Type>, Type)> {
    let Type::Id(id) = ty else {
        bail!("not a result type")
    };
    match &resolve.types[id].kind {
        TypeDefKind::Type(inner) => result_halves(resolve, *inner),
        TypeDefKind::Result(r) => {
            let err = r.err.context("a result with no error half")?;
            Ok((r.ok, err))
        }
        kind => bail!("{} is not a result type", kind.as_str()),
    }
}

/// Canonical-ABI numbers for the return area, plus the bump-pointer
/// reservation the adapter takes for it.
fn layout(
    resolve: &Resolve,
    result: Type,
    sig: &wit_parser::abi::WasmSignature,
    param: &Repr,
    ok: Type,
    err: Type,
) -> Result<Layout> {
    // A result that flattens to one core value is returned in a register, with
    // no return area at all -- a different adapter shape than the one emitted
    // here. Nothing in the supported subset does that (`result<T, error>` is
    // always two values, a discriminant and a payload), so this is a guard
    // against a future widening rather than a case to handle.
    ensure!(
        sig.retptr,
        "this export's result flattens to {} core value(s), so the canonical ABI \
         returns it in a register rather than through a return area -- and the \
         generated adapter is written around a return area. Nothing in the \
         supported subset does this: `result<T, error>` is always a discriminant \
         plus a payload.",
        sig.results.len()
    );
    ensure!(
        !sig.indirect_params,
        "the parameters are passed indirectly, which only happens above {} flat \
         values and the gate allows one parameter",
        wit_parser::Resolve::MAX_FLAT_PARAMS
    );

    let mut sizes = SizeAlign::default();
    sizes.fill(resolve);

    let ret_size = sizes.size(&result).size_wasm32() as u32;
    let ret_align = sizes.align(&result).align_wasm32() as u32;
    // `result<T, E>` is a two-case variant, so a one-byte discriminant; the
    // payload follows it, aligned to the wider of the two halves.
    let ret_tag = Int::U8;
    let payload = sizes
        .payload_offset(ret_tag, [Some(&ok), Some(&err)])
        .size_wasm32() as u32;

    // The return area has to be 8-aligned in the worst case and `mem_ptr` is
    // only 4-aligned, so the adapter aligns it up; reserving a multiple of 8
    // then leaves `mem_ptr` where the backend's 4-byte cells want it.
    ensure!(
        ret_align <= ADAPTER_ALIGN,
        "the return area wants {ret_align}-byte alignment, more than the \
         {ADAPTER_ALIGN} the adapter aligns `mem_ptr` to"
    );
    let area = ret_size.next_multiple_of(ADAPTER_ALIGN);
    let (reserve, arg) = if param.boxed() {
        (area + repr::PRIM_PAYLOAD_SIZE, Some(area))
    } else {
        (area, None)
    };

    Ok(Layout {
        ret_size,
        ret_align,
        payload,
        ret_tag,
        err_tag: enum_tag(resolve, err)?,
        reserve,
        arg,
    })
}

/// What the adapter aligns the return area, and its reservation, to.
pub const ADAPTER_ALIGN: u32 = 8;

fn enum_tag(resolve: &Resolve, ty: Type) -> Result<Int> {
    let Type::Id(id) = ty else {
        bail!("not an enum")
    };
    match &resolve.types[id].kind {
        TypeDefKind::Type(inner) => enum_tag(resolve, *inner),
        TypeDefKind::Enum(e) => Ok(e.tag()),
        kind => bail!("{} is not an enum", kind.as_str()),
    }
}

/// The discriminants the adapter stores, by name rather than by position.
fn discriminants(resolve: &Resolve, err: Type, param: &Repr) -> Result<Discriminants> {
    let cases = crate::error_case_names(resolve, err)?;
    let case = |name: &str| -> Result<u32> {
        cases
            .iter()
            .position(|c| c == name)
            .map(|i| i as u32)
            .with_context(|| {
                format!(
                    "the error enum has no `{name}` case; it declares {cases:?}. An enum's \
                     case order is its discriminant, so the adapter cannot guess one -- see \
                     TODO.md#abi-package-version"
                )
            })
    };
    Ok(Discriminants {
        // `result<T, E>` declares ok first, so ok is 0 and err is 1. Fixed by
        // the component model, not by us.
        ok: 0,
        err: 1,
        out_of_memory: case("out-of-memory")?,
        out_of_range: match param.boxed() {
            true => Some(case("out-of-range")?),
            false => None,
        },
    })
}

/// Every way the gate says this call can fail must have somewhere to go.
fn check_failures(failures: &[Failure], param: &Repr, discriminants: &Discriminants) -> Result<()> {
    for failure in failures {
        match failure {
            // Unconditional, and `discriminants` found its case by name or
            // failed there; nothing left to check.
            Failure::OutOfMemory => {}
            Failure::OutOfRange(what) => ensure!(
                param.boxed() && discriminants.out_of_range.is_some(),
                "the gate reports that this export can fail with out of range ({what}), \
                 but the adapter emitted for a {} parameter has no range check. Teach \
                 the adapter that case, or narrow `support::failure_modes`",
                param.rocq_type()
            ),
        }
    }
    // And the converse: a range check with nothing to report through would
    // store a discriminant the world never declared.
    if discriminants.out_of_range.is_some() {
        ensure!(
            failures.iter().any(|f| matches!(f, Failure::OutOfRange(_))),
            "the adapter would emit a range check for a failure the gate does not list"
        );
    }
    Ok(())
}

/// The single core parameter type, as it is written in wat.
fn wasm_type(params: &[WasmType], export: &str) -> Result<&'static str> {
    ensure!(
        params.len() == 1,
        "`{export}` flattens to {} core parameters; the generated adapter takes one",
        params.len()
    );
    match params[0] {
        WasmType::I32 | WasmType::Pointer | WasmType::Length => Ok("i32"),
        WasmType::I64 => Ok("i64"),
        other => bail!("a {other:?} parameter is not carried -- see TODO.md"),
    }
}

/// How the export reads in the WIT file, for the comment the generated files
/// open with. Rebuilt from the resolve rather than quoted from the source, so
/// it says what was understood rather than what was written.
fn signature(resolve: &Resolve, name: &str, func: &Function) -> String {
    let params = func
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, show(resolve, p.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    match func.result {
        Some(ty) => format!("export {name}: func({params}) -> {}", show(resolve, ty)),
        None => format!("export {name}: func({params})"),
    }
}

fn show(resolve: &Resolve, ty: Type) -> String {
    match ty {
        Type::Id(id) => {
            let def = &resolve.types[id];
            if let Some(name) = &def.name {
                return name.clone();
            }
            match &def.kind {
                TypeDefKind::Result(r) => format!(
                    "result<{}, {}>",
                    r.ok.map(|t| show(resolve, t)).unwrap_or_default(),
                    r.err.map(|t| show(resolve, t)).unwrap_or_default()
                ),
                TypeDefKind::Option(t) => format!("option<{}>", show(resolve, *t)),
                TypeDefKind::Type(t) => show(resolve, *t),
                kind => kind.as_str().to_string(),
            }
        }
        Type::Bool => "bool".into(),
        Type::U8 => "u8".into(),
        Type::U16 => "u16".into(),
        Type::U32 => "u32".into(),
        Type::U64 => "u64".into(),
        Type::S8 => "s8".into(),
        Type::S16 => "s16".into(),
        Type::S32 => "s32".into(),
        Type::S64 => "s64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::Char => "char".into(),
        Type::String => "string".into(),
        Type::ErrorContext => "error-context".into(),
    }
}

/// File names, from the config, as the build rules need them.
fn build(cfg: &Config, wit_dir: &str) -> Result<Build> {
    let stem = |path: &std::path::Path, what: &str| -> Result<String> {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .with_context(|| format!("{what} `{}` has no usable file stem", path.display()))?;
        // The stem is the Rocq library name, so `Require Import <stem>` has to
        // be an identifier.
        ensure!(
            !stem.is_empty()
                && stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !stem.starts_with(|c: char| c.is_ascii_digit()),
            "{what} `{}` is not a Rocq library name: `Require Import {stem}` would not \
             parse. Rename the file.",
            path.display()
        );
        Ok(stem.to_string())
    };

    Ok(Build {
        wasm_name: cfg.wasm.name.clone(),
        interface_stem: INTERFACE_STEM.to_string(),
        impl_stem: stem(&cfg.impl_.file, "the implementation file")?,
        extraction_stem: EXTRACTION_STEM.to_string(),
        impl_module: cfg.impl_.module.clone(),
        wit_dir: wit_dir.to_string(),
        flags: cfg.certirocq.flags.clone(),
    })
}

/// Stems of the two generated Rocq files. These are `Require Import`ed by name,
/// so they are identifiers, not configuration.
pub const INTERFACE_STEM: &str = "interface";
pub const EXTRACTION_STEM: &str = "extraction";
