//! A module that runs every conversion in [`super::convert`] both ways, so
//! `checks/roundtrip.py` can require the value to come back unchanged.
//!
//! This is not part of a component build and `generate` never writes it. It
//! exists because the conversions are a *pair*, and an export adapter only
//! ever exercises each direction in one position: a parameter goes canonical
//! -> Rocq, a result goes Rocq -> canonical. An import wrapper will need them
//! the other way round (TODO.md#imports), and "the shift inverts the shift"
//! is exactly the kind of claim that is obviously true until a representation
//! moves underneath it.
//!
//! So each representation gets two exported functions, each composing the pair
//! in one order, and each expected to be the identity:
//!
//! ```text
//!   canonical_<type>   v  ->  to_rocq    ->  from_rocq  ->  v
//!   rocq_<type>        x  ->  from_rocq  ->  to_rocq    ->  x
//! ```
//!
//! The identity is the whole expectation, which is why the values are emitted
//! alongside rather than written out in the check: a round trip has nothing to
//! compare against but its own input. What the check does restate is the
//! boundaries it will not run without -- see `checks/roundtrip.py`.
//!
//! **What is not here.** A sum -- a WIT `option`, or a `result` in a value
//! position. Its canonical form is a discriminant and a payload at byte
//! offsets, not one core value, so "the same function in both orders, each
//! taking and returning a value" cannot be written for it: the export would
//! have to return two things. Running those both ways needs a caller that can
//! read memory, which is `checks/backend.py` driving a real component, and
//! that is where they are run. The import direction of a sum therefore has no
//! evidence here -- see TODO.md#imports.
//!
//! **Where it is the identity.** On the values the type actually has. A
//! canonical `bool` is 0 or 1; the canonical ABI lifts *any* non-zero byte as
//! `true`, so `to_rocq` normalises 2 to `true` and the trip back gives 1
//! rather than 2. That is the conversion being right, not a round trip
//! failing, so the emitted values are the type's own.

use std::fmt::Write;

use serde::Serialize;

use super::plan::{Cells, Plan, Repr};
use super::{comment, convert, MARKER};
use crate::repr;

/// Where the two boxes a boxed round trip needs live, in the harness's own
/// memory. Two, not one: storing twice into the same cell and reading it back
/// would pass even if the second store never happened.
const BOX_A: u32 = 0;
const BOX_B: u32 = 16;

/// What the check needs to know to run one representation's round trip.
#[derive(Debug, Clone, Serialize)]
pub struct Pair {
    /// The Rocq type, which names both exports.
    pub rocq_type: String,
    /// The export going canonical -> Rocq -> canonical.
    pub canonical: String,
    /// The export going Rocq -> canonical -> Rocq.
    pub rocq: String,
    /// The core wasm type both take and return.
    pub core: &'static str,
    /// Values on the canonical side the pair must be the identity on.
    pub canonical_values: Vec<String>,
    /// The same trip from the other side: immediates for a nullary
    /// representation, payloads for a boxed one.
    pub rocq_values: Vec<String>,
}

/// A type the harness has no round trip for.
///
/// In the manifest so that a world carrying nothing else -- `option<u64>` in
/// and out, say -- is distinguishable from a generator that emitted nothing:
/// one is a harness with nothing to run, the other is a bug, and an empty
/// manifest alone cannot tell the check which it is looking at.
#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    /// How the type reads in the world.
    pub wit: String,
    /// The Rocq type carrying it.
    pub rocq_type: String,
    /// Why there is no round trip, and what runs the conversion instead.
    pub reason: &'static str,
}

/// The harness module, and the manifest describing what to call in it.
pub fn module(plan: &Plan) -> (String, Vec<Pair>) {
    // One pair per distinct representation the world carries, parameters and
    // result together: the conversion is a property of the representation, not
    // of the position it appears in.
    let mut pairs: Vec<(Pair, Repr)> = Vec::new();
    for repr in plan
        .export
        .params
        .iter()
        .map(|p| &p.repr)
        .chain([&plan.export.ok])
    {
        // Sums and records are not here, and the omission is the point of
        // the note in the module docs above: a round trip through one core
        // value cannot express a value whose canonical form is several of
        // them -- a discriminant and a payload at byte offsets, or a field
        // per cell. `checks/backend.py` runs those both ways against the real
        // backend instead.
        //
        // A one-case `enum` is not here either, for a different reason. It has
        // one core value, but its lift is a constant -- the only discriminant
        // the type has, read off the type rather than off the value
        // (TODO.md#enum-unit) -- so composing the two conversions is the
        // identity whatever the lowering emits, and the trip would pass on a
        // lowering that was wrong. The immediate it would be run with is not a
        // value the backend produces either: it erases the constructor and
        // allocates. `checks/backend.py` carries the case against the real
        // backend instead.
        if matches!(
            repr,
            Repr::Sum(_) | Repr::Record(_) | Repr::Enum { cases: 1, .. }
        ) {
            continue;
        }
        let this = pair(repr);
        if !pairs.iter().any(|(p, _)| p.rocq_type == this.rocq_type) {
            pairs.push((this, repr.clone()));
        }
    }

    let mut out = comment::wat(
        "",
        &[
            format!(
                "{MARKER} --roundtrip wrote this from the world `{}`:",
                plan.world_name
            ),
            String::new(),
            format!("    {}", plan.export.signature),
            String::new(),
            "It is not part of a component build and `generate` never writes".into(),
            "it. Each pair of exports composes the two conversions in".into(),
            "bindgen/src/emit/convert.rs in one order, and each is expected to".into(),
            "be the identity -- which is the property an import wrapper will".into(),
            "rest on, established before there is one. checks/roundtrip.py".into(),
            "runs it.".into(),
        ],
    );
    out.push_str("(module\n");
    out.push_str(&comment::wat(
        "  ",
        &[
            "The harness's own memory: nothing here links against the backend,".to_string(),
            format!(
                "since a conversion only ever touches memory and arithmetic. The {}",
                repr::PRIM_PAYLOAD_SIZE
            ),
            "bytes a boxed payload needs are at two addresses, so a trip that".into(),
            "forgets its second store cannot pass by reading the first one".into(),
            "back. Both arrive dirty, for the same reason the stubs under".into(),
            "examples/ do: over a zeroed page a forgotten store reads as 0,".into(),
            "which is a value the round trip is run with.".into(),
        ],
    ));
    out.push_str("  (memory (export \"memory\") 1)\n");
    writeln!(out, "  (global $box_a i32 (i32.const {BOX_A}))").unwrap();
    writeln!(out, "  (global $box_b i32 (i32.const {BOX_B}))").unwrap();
    writeln!(
        out,
        "  (data (i32.const {BOX_A}) \"{}\")\n",
        "\\a5".repeat((BOX_B + repr::PRIM_PAYLOAD_SIZE - BOX_A) as usize)
    )
    .unwrap();

    for (pair, repr) in &pairs {
        emit_pair(&mut out, pair, repr);
    }
    out.push_str(")\n");
    (out, pairs.into_iter().map(|(p, _)| p).collect())
}

/// The manifest `checks/roundtrip.py` reads: what to call in the module, with
/// what, and what to expect back.
///
/// TOML rather than JSON because the repository already depends on a TOML
/// parser on both sides -- `toml` here, `tomllib` in the check -- and a
/// manifest format is not worth a dependency.
///
/// The expected value is never in it. A round trip has nothing to compare
/// against but its own input, so the check compares against what it sent; a
/// manifest that also carried the answer could agree with a broken generator
/// about both.
pub fn manifest(plan: &Plan, pairs: &[Pair]) -> String {
    #[derive(Serialize)]
    struct Manifest<'a> {
        world: &'a str,
        signature: &'a str,
        pair: &'a [Pair],
        skipped: Vec<Skipped>,
    }

    let doc = toml::to_string_pretty(&Manifest {
        world: &plan.world_name,
        signature: &plan.export.signature,
        pair: pairs,
        skipped: skipped(plan),
    })
    .expect("a manifest of strings and numbers always serialises");

    format!(
        "{}\n{doc}",
        super::comment::make(&[
            format!("{MARKER} --roundtrip wrote this."),
            String::new(),
            "What to call in the harness module next to it, with what, and".to_string(),
            "what to expect back -- which is always the value sent, since a".to_string(),
            "round trip is expected to be the identity. checks/roundtrip.py".to_string(),
            "reads it.".to_string(),
        ])
    )
}

/// The types `module` passed over, in the order the world mentions them and
/// deduplicated by Rocq type, as the pairs are.
fn skipped(plan: &Plan) -> Vec<Skipped> {
    let mut out: Vec<Skipped> = Vec::new();
    for repr in plan
        .export
        .params
        .iter()
        .map(|p| &p.repr)
        .chain([&plan.export.ok])
    {
        let (wit, rocq_type, reason) = match repr {
            Repr::Sum(sum) => (
                sum.wit.clone(),
                sum.rocq_type.clone(),
                "a discriminant and a payload at byte offsets is not one core value, \
                 so there is no function to compose with itself; checks/backend.py \
                 runs both directions against the real backend instead",
            ),
            Repr::Record(rec) => (
                rec.wit.clone(),
                rec.rocq_type.clone(),
                "a field per cell is not one core value, so there is no function to \
                 compose with itself; checks/backend.py runs both directions against \
                 the real backend instead",
            ),
            Repr::Enum {
                rocq_type,
                cases: 1,
                ..
            } => (
                plan.inductives
                    .iter()
                    .find(|i| &i.rocq_name == rocq_type)
                    .expect("every enum representation declared its inductive")
                    .wit_name
                    .clone(),
                rocq_type.clone(),
                "its lift answers with the only discriminant the type has and never \
                 reads the value (TODO.md#enum-unit), so composing the two \
                 conversions is the identity whatever the lowering emits; \
                 checks/backend.py runs both directions against the real backend \
                 instead",
            ),
            _ => continue,
        };
        if out.iter().any(|s| s.rocq_type == rocq_type) {
            continue;
        }
        out.push(Skipped {
            wit,
            rocq_type,
            reason,
        });
    }
    out
}

fn emit_pair(out: &mut String, pair: &Pair, repr: &Repr) {
    let core = pair.core;

    // canonical -> Rocq -> canonical.
    out.push_str(&comment::wat(
        "  ",
        &[format!(
            "A canonical `{}` into the backend's representation and back.",
            pair.rocq_type
        )],
    ));
    writeln!(
        out,
        "  (func (export \"{}\") (param $v {core}) (result {core})",
        pair.canonical
    )
    .unwrap();
    let there = convert::to_rocq(repr, &[sent(repr)], &cells(repr), box_a());
    for line in &there.prologue {
        writeln!(out, "    {line}").unwrap();
    }
    let back = convert::from_rocq(repr, &there.value);
    for line in &back.prologue {
        writeln!(out, "    {line}").unwrap();
    }
    writeln!(out, "    {})\n", back.value).unwrap();

    // Rocq -> canonical -> Rocq.
    out.push_str(&comment::wat(
        "  ",
        &match repr {
            Repr::Int63 => vec![
                format!(
                    "A boxed `{}` out to the canonical ABI and back.",
                    pair.rocq_type
                ),
                "The value is the payload: the box itself is an address, which".into(),
                "says nothing from outside the module, so what goes in is".into(),
                "stored into the first box and what comes out is read from the".into(),
                "second.".into(),
            ],
            Repr::Bool | Repr::Enum { .. } => vec![
                format!(
                    "A `{}` immediate -- `2*ord+1`, as the backend leaves it --",
                    pair.rocq_type
                ),
                "out to the canonical ABI and back.".into(),
            ],
            Repr::Sum(sum) => unreachable!("`{}` has no single-value round trip", sum.wit),
            Repr::Record(rec) => unreachable!("`{}` has no single-value round trip", rec.wit),
        },
    ));
    writeln!(
        out,
        "  (func (export \"{}\") (param $v {core}) (result {core})",
        pair.rocq
    )
    .unwrap();
    // The Rocq-side value to start from. For a boxed representation that
    // means building one, which is `to_rocq` -- setup, not the trip.
    let start = match repr {
        Repr::Sum(sum) => unreachable!("`{}` has no single-value round trip", sum.wit),
        Repr::Record(rec) => unreachable!("`{}` has no single-value round trip", rec.wit),
        Repr::Int63 => {
            let build = convert::to_rocq(repr, &[sent(repr)], &cells(repr), box_a());
            for line in &build.prologue {
                writeln!(out, "    {line}").unwrap();
            }
            build.value
        }
        Repr::Bool | Repr::Enum { .. } => "(local.get $v)".to_string(),
    };
    let out_ = convert::from_rocq(repr, &start);
    for line in &out_.prologue {
        writeln!(out, "    {line}").unwrap();
    }
    let home = convert::to_rocq(
        repr,
        &[convert::Flat {
            value: out_.value.clone(),
            wasm: convert::core_type(repr),
        }],
        &cells(repr),
        box_b(),
    );
    for line in &home.prologue {
        writeln!(out, "    {line}").unwrap();
    }
    // What a caller can compare: the payload for a boxed representation, the
    // immediate itself for the others.
    let observable = match repr {
        Repr::Int63 => format!("(i64.load {})", home.value),
        Repr::Bool | Repr::Enum { .. } => home.value.clone(),
        Repr::Sum(sum) => unreachable!("`{}` has no single-value round trip", sum.wit),
        Repr::Record(rec) => unreachable!("`{}` has no single-value round trip", rec.wit),
    };
    writeln!(out, "    {observable})\n").unwrap();
}

/// The harness's parameter, as the conversion reads it.
fn sent(repr: &Repr) -> convert::Flat {
    convert::Flat {
        value: "(local.get $v)".into(),
        wasm: convert::core_type(repr),
    }
}

/// Where a boxed payload goes. The harness has one cell per box and nothing
/// nested to put in it, since a sum never gets here.
fn cells(repr: &Repr) -> Cells {
    match repr {
        Repr::Int63 => Cells::Payload(0),
        _ => Cells::Nothing,
    }
}

fn box_a() -> convert::Base<'static> {
    convert::Base("(global.get $box_a)")
}

fn box_b() -> convert::Base<'static> {
    convert::Base("(global.get $box_b)")
}

/// The values a representation's round trip is run with, and the names of its
/// two exports.
fn pair(repr: &Repr) -> Pair {
    let rocq_type = repr.rocq_type().to_string();
    let (canonical_values, rocq_values) = match repr {
        // The whole range `Uint63.int` holds, ends included. 2^63 and above is
        // not here on purpose: it has no `Uint63.int`, which is why the
        // adapter range-checks it and answers `out-of-range` instead of
        // converting it (TODO.md#failure-channel).
        Repr::Int63 => {
            let values: Vec<String> =
                ["0", "1", "42", "4611686018427387903", "9223372036854775807"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
            // The payload is the same number on both sides; what differs is
            // which conversion runs first.
            (values.clone(), values)
        }
        // A canonical `bool` is 0 or 1. Not 2: the canonical ABI lifts any
        // non-zero byte as `true`, so the trip normalises it rather than
        // returning it, and that is the conversion being right.
        Repr::Bool => (
            vec!["0".into(), "1".into()],
            vec![repr::BOOL_FALSE.to_string(), repr::BOOL_TRUE.to_string()],
        ),
        // Every case, because the immediate is per case and a shift that was
        // right for the first one can be wrong for the last.
        // `module` filters this one out too: see the note there.
        Repr::Enum {
            cases: 1,
            rocq_type,
            ..
        } => unreachable!("`{rocq_type}` has a round trip that cannot fail"),
        Repr::Enum { cases, .. } => (
            (0..*cases).map(|d| d.to_string()).collect(),
            (0..*cases as u32)
                .map(|d| repr::nullary_immediate(d).to_string())
                .collect(),
        ),
        // `module` filters these out before getting here.
        Repr::Sum(sum) => unreachable!("`{}` has no single-value round trip", sum.wit),
        Repr::Record(rec) => unreachable!("`{}` has no single-value round trip", rec.wit),
    };
    Pair {
        canonical: format!("canonical_{rocq_type}"),
        rocq: format!("rocq_{rocq_type}"),
        core: convert::core_type(repr),
        rocq_type,
        canonical_values,
        rocq_values,
    }
}
