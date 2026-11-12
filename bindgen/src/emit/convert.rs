//! The two conversions between CertiRocq's values and the canonical ABI's,
//! and the fact that they are inverses.
//!
//! An export adapter and an import wrapper need the same pair of conversions
//! in opposite orders. An export takes its parameters from the canonical ABI
//! into the backend's representation, and its result back out: [`to_rocq`]
//! then [`from_rocq`] or [`store_canonical`]. An import wrapper, when there is
//! one (TODO.md#imports), does exactly the reverse -- it takes the Rocq
//! arguments the compiled program handed it out into core values to pass to
//! the host, and takes the value that comes back in.
//!
//! So the *order* is the caller's business and the conversion is not. Both
//! directions live here, each emitted from one place, and
//! `checks/roundtrip.py` runs every scalar pair in both orders through
//! `wasmtime` and requires the value to come back unchanged. That is the
//! property an import wrapper will rest on, and it can be established before
//! there is one -- the gate still refuses imports (`E-IMPORT`).
//!
//! Nothing here allocates. A value that needs heap cells is given their
//! address; where the address comes from differs between the two directions --
//! an export reserves every cell up front, off the bump pointer, before the
//! program runs (TODO.md#arity) -- and an import wrapper would have to find
//! one mid-call, which is a question about imports rather than about the
//! conversion.
//!
//! **Two shapes, not one.** A scalar is one core value in both
//! representations, so its conversion is an expression ([`from_rocq`]). A sum
//! is not: the canonical ABI carries it as a discriminant and a payload, in
//! several flat values as a parameter and at byte offsets in memory as a
//! result. So the Rocq-to-canonical direction of a sum is a *store*
//! ([`store_canonical`]) rather than a value, and the canonical-to-Rocq
//! direction reads several slots instead of one.

use std::fmt::Write;

use super::plan::{Cells, CtorTag, Record, Repr, Slot, Sum, SumCase};
use crate::repr;

/// One flat value of the canonical ABI: the expression yielding it, and the
/// core type it is carried in.
///
/// An expression rather than a local, because the two callers differ. An
/// export adapter reads a parameter out of the local it arrives in; the
/// round-trip harness lowers a value it has just computed, which is in no
/// local at all. What the conversion needs is the same either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flat {
    pub value: String,
    pub wasm: &'static str,
}

impl Flat {
    /// The flat values a parameter arrives in, read out of their locals.
    pub fn locals(slots: &[Slot]) -> Vec<Flat> {
        slots
            .iter()
            .map(|slot| Flat {
                value: format!("(local.get ${})", slot.local),
                wasm: slot.wasm,
            })
            .collect()
    }
}

/// Statements to emit before the value can be read, then the expression that
/// holds it.
///
/// The prologue is empty for every conversion that is pure arithmetic. It is
/// the store into a cell for the ones that are not, which is why these cannot
/// just be expressions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    pub prologue: Vec<String>,
    pub value: String,
}

impl Conversion {
    fn pure(value: String) -> Conversion {
        Conversion {
            prologue: Vec::new(),
            value,
        }
    }
}

/// An address and the offsets reached from it: the heap cells a lowering
/// writes into, or the block a lift reads a constructor's fields out of.
///
/// Offset zero is spelled as the bare base rather than as an `i32.add` of
/// nothing, and an access at offset zero carries no `offset=`. Not cosmetic:
/// that is what the hand-written `examples/plus1` adapter writes for its one
/// box, and `checks/generated.py` holds this emitter to that file token for
/// token.
#[derive(Debug, Clone, Copy)]
pub struct Base<'a>(pub &'a str);

impl Base<'_> {
    /// The address of the cell at `offset`.
    pub fn addr(&self, offset: u32) -> String {
        match offset {
            0 => self.0.to_string(),
            n => format!("(i32.add {} (i32.const {n}))", self.0),
        }
    }

    /// A store of `value` into the cell at `offset`.
    pub fn store(&self, op: &str, offset: u32, value: &str) -> String {
        match offset {
            0 => format!("({op} {} {value})", self.0),
            n => format!("({op} offset={n} {} {value})", self.0),
        }
    }

    /// A load from the cell at `offset`.
    pub fn load(&self, op: &str, offset: u32) -> String {
        match offset {
            0 => format!("({op} {})", self.0),
            n => format!("({op} offset={n} {})", self.0),
        }
    }
}

/// The core wasm type a value of this representation is carried in on the
/// canonical ABI side.
///
/// Only meaningful for the types that *are* one core value. A sum is several
/// ([`Repr::flatten`]), and asking for the one is a bug in the caller.
pub fn core_type(repr: &Repr) -> &'static str {
    match repr {
        Repr::Int63 => "i64",
        Repr::Bool | Repr::Enum { .. } => "i32",
        Repr::Sum(sum) => unreachable!("`{}` is carried in several core values, not one", sum.wit),
        Repr::Record(rec) => unreachable!(
            "`{}` is carried in {} core value(s), not one",
            rec.wit,
            rec.fields.len()
        ),
    }
}

/// One flat value, at the width the case reading it wants.
///
/// The cast is the join doing its work. The canonical ABI gives both cases of
/// a sum the same slots, widened to hold either, so a `bool` payload sharing a
/// slot with a `u64` one arrives in an i64 and is read back out of its low
/// half. Widening is the only direction that can happen -- the join never
/// narrows -- so a slot that is too small for what is wanted is a bug here
/// rather than a case to handle.
fn at_width(slot: &Flat, want: &str) -> String {
    match (slot.wasm, want) {
        (have, want) if have == want => slot.value.clone(),
        ("i64", "i32") => format!("(i32.wrap_i64 {})", slot.value),
        (have, want) => unreachable!(
            "the flat value {} arrives as {have} and is wanted as {want}; the canonical \
             ABI's join only ever widens",
            slot.value
        ),
    }
}

/// A canonical-ABI value, as the value CertiRocq represents it with.
///
/// `slots` are the flat values this type arrives in, in canonical order --
/// exactly as many as [`Repr::flatten`] gives. `cells` and `base` say where it
/// may write; both are ignored by a representation that is an immediate, and a
/// mismatch between them and `repr` is a bug in the caller rather than a
/// condition to report: there is no code to emit without somewhere to put a
/// payload.
///
/// `(d << 1) | 1` is `2*ord+1` with the ordinal equal to the discriminant,
/// which holds for an `enum` because the generator writes the inductive in the
/// world's case order, and for `bool` because the pipeline's swap
/// (`CertiRocq.Plugin.Loader`: `bool => "bool" [ 1 0 ]`) happens to line its
/// ordinals up with the component model's `false = 0, true = 1`. Two reasons,
/// one shift -- and [`Repr::tags_are_the_formula`] is what keeps it honest if
/// either reason stops holding.
pub fn to_rocq(repr: &Repr, slots: &[Flat], cells: &Cells, base: Base) -> Conversion {
    match repr {
        // Boxed: the payload is stored, and what gets passed is its address.
        Repr::Int63 => {
            let Cells::Payload(at) = cells else {
                unreachable!("a primitive int needs a payload cell to store its value in")
            };
            Conversion {
                prologue: vec![base.store("i64.store", *at, &at_width(&slots[0], "i64"))],
                value: base.addr(*at),
            }
        }
        // `i32.ne` first: the canonical ABI lifts any non-zero byte as `true`,
        // and a 2 shifted left would name a third constructor `bool` has not
        // got.
        Repr::Bool => Conversion::pure(format!(
            "(i32.or (i32.shl (i32.ne {} (i32.const 0)) (i32.const 1)) (i32.const 1))",
            at_width(&slots[0], "i32")
        )),
        // The shift, for a one-case enum too, where [`from_rocq`] is a
        // constant instead. Nothing on the Rocq side reads this value -- a
        // constructor that distinguishes nothing has no projection and no
        // test (TODO.md#enum-unit) -- and the immediate is what the value
        // would be if the backend ever stopped erasing it, so sending it is
        // right in the one case that can be read and harmless in the other.
        Repr::Enum { .. } => Conversion::pure(format!(
            "(i32.or (i32.shl {} (i32.const 1)) (i32.const 1))",
            at_width(&slots[0], "i32")
        )),
        Repr::Sum(sum) => lower_sum(sum, slots, cells, base),
        Repr::Record(rec) => lower_record(rec, slots, cells, base),
    }
}

/// A record, built from its fields' flat values.
///
/// No branch, unlike a sum: a record has one constructor, so there is nothing
/// to select and the value is always the block's address. What it does have
/// is an *order*. Every field's own value goes down first, because the cell
/// pointing at it cannot be written before there is something to point at;
/// then the ordinal, then the cells. The ordinal is written rather than left
/// to whatever the heap held -- the compiled projections load it and trap on
/// anything else, which is a thing an adapter tested over a zeroed page would
/// never find out.
fn lower_record(rec: &Record, slots: &[Flat], cells: &Cells, base: Base) -> Conversion {
    // No fields is a nullary constructor, so the value *is* the immediate and
    // no cell is involved. The same number `None` is, for the same reason.
    if rec.fields.is_empty() {
        return Conversion::pure(format!(
            "(i32.const {})",
            repr::nullary_immediate(repr::SINGLE_CTOR_ORDINAL)
        ));
    }
    let Cells::Block { at, inner } = cells else {
        unreachable!("a record needs a block to build its constructor in")
    };

    let mut prologue = Vec::new();
    let mut pointers = Vec::new();
    let mut rest = slots;
    for (i, (field, cells)) in rec.fields.iter().zip(inner).enumerate() {
        // Each field takes its own flat values off the front: the canonical
        // ABI flattens a record by concatenating them, so the split is the
        // field's own flattening rather than a count of fields.
        let take = flat_count(&field.repr);
        let built = to_rocq(&field.repr, &rest[..take], cells, base);
        rest = &rest[take..];
        prologue.extend(built.prologue);
        pointers.push(base.store("i32.store", at + rec.cell(i), &built.value));
    }
    prologue.push(base.store(
        "i32.store",
        at + repr::BOXED_ORDINAL_OFFSET,
        &format!("(i32.const {})", repr::SINGLE_CTOR_ORDINAL),
    ));
    prologue.extend(pointers);
    Conversion {
        prologue,
        value: base.addr(*at),
    }
}

/// How many flat values a type takes, for splitting a record's slots between
/// its fields.
///
/// `Repr::flatten` can refuse a join it has no rule for, and did not: the
/// plan was built before any of this is emitted, and building it flattens
/// every type in the signature. So a failure here is a bug rather than a
/// condition.
fn flat_count(repr: &Repr) -> usize {
    repr.flatten()
        .expect("the plan flattened this type when it was built")
        .len()
}

/// A two-case sum, built from the discriminant and the payload slots.
///
/// One `if` with a value, because exactly one of the two constructors is
/// built: the discriminant selects it, and each branch ends with the value
/// that branch produces -- an immediate for a case with no payload, the
/// address of the `[ord, field]` block for a case with one.
///
/// The two cases *share* the block, since only one of them is ever written,
/// and share their flat slots, since that is what the canonical ABI's join
/// means. What they do not share is their payload cells: those are reserved
/// separately ([`Repr::cells`]).
fn lower_sum(sum: &Sum, slots: &[Flat], cells: &Cells, base: Base) -> Conversion {
    let Cells::Block { at, inner } = cells else {
        unreachable!("a sum needs a block to build its constructor in")
    };
    let discriminant = at_width(&slots[0], "i32");

    let mut branches = Vec::new();
    for (case, cells) in sum.cases.iter().zip(inner) {
        let mut body = Vec::new();
        let value = match (&case.payload, case.tag) {
            // Nothing to build: the constructor *is* the immediate.
            (None, CtorTag::Immediate(immediate)) => format!("(i32.const {immediate})"),
            // `[ord, field]`: the payload is lowered first, because its own
            // store has to happen before the field pointing at it is written.
            (Some(payload), CtorTag::Boxed(ordinal)) => {
                let inner = to_rocq(payload, &slots[1..], cells, base);
                body.extend(inner.prologue);
                body.push(base.store(
                    "i32.store",
                    at + repr::BOXED_ORDINAL_OFFSET,
                    &format!("(i32.const {ordinal})"),
                ));
                body.push(base.store("i32.store", at + repr::boxed_field_offset(0), &inner.value));
                base.addr(*at)
            }
            // The table on `Sum` pairs a payload with a boxed constructor and
            // an empty case with an immediate one; nothing builds any other
            // combination.
            (payload, tag) => unreachable!(
                "case `{}` carries {} and is represented as {tag:?}",
                case.wit_name,
                match payload {
                    Some(repr) => repr.rocq_type(),
                    None => "nothing".into(),
                }
            ),
        };
        body.push(value);
        branches.push(body);
    }

    let mut value = format!("(if (result i32) (i32.eqz {discriminant})\n");
    for (keyword, body) in [("then", &branches[0]), ("else", &branches[1])] {
        writeln!(value, "  ({keyword}").unwrap();
        for line in body.iter() {
            // A statement can itself be several lines -- a nested sum is
            // another `if`, built where the field pointing at it is written --
            // so its continuations are shifted with it. Once per level, which
            // is what makes a sum three deep read as three `if`s.
            writeln!(value, "    {}", line.replace('\n', "\n    ")).unwrap();
        }
        value.push_str("  )\n");
    }
    value.push(')');
    Conversion::pure(value)
}

/// A CertiRocq value, as the canonical ABI's core value.
///
/// The inverse of [`to_rocq`] for the representations that are a single core
/// value: a boxed value is read back through its address, and `2*ord+1`
/// inverts to a shift. Both are total on the values the other direction can
/// produce, which is what `checks/roundtrip.py` runs.
///
/// A sum has no single core value to be read into, so it goes through
/// [`store_canonical`] instead.
pub fn from_rocq(repr: &Repr, value: &str) -> Conversion {
    match repr {
        Repr::Int63 => Conversion::pure(format!("(i64.load {value})")),
        // A one-case enum carries no information, so the discriminant is 0
        // whatever the value is -- and the value is the one place this cannot
        // look. The backend erases a constructor that distinguishes nothing:
        // there is no projection for a field of such a type and no test in a
        // `match` on it, and the constructor itself is built as a heap block
        // rather than given the nullary immediate, so `2*ord+1` does not hold
        // and halving it yields half an address. Answering with the only
        // discriminant the type has is right under either representation --
        // see TODO.md#enum-unit, and `checks.representation` measures which
        // one the backend is on today.
        Repr::Enum { cases: 1, .. } => Conversion::pure("(i32.const 0)".to_string()),
        Repr::Bool | Repr::Enum { .. } => {
            Conversion::pure(format!("(i32.shr_u {value} (i32.const 1))"))
        }
        Repr::Sum(sum) => unreachable!(
            "`{}` is a discriminant and a payload at byte offsets, not a core value; \
             use store_canonical",
            sum.wit
        ),
        Repr::Record(rec) => unreachable!(
            "`{}` is its fields at byte offsets, not a core value; use store_canonical",
            rec.wit
        ),
    }
}

/// Statements writing a value into memory the way the canonical ABI reads it
/// back.
///
/// The other half of the Rocq-to-canonical direction, for a value whose
/// canonical form lives at byte offsets rather than in a register. `dest` is
/// the area and `offset` where this value starts in it; every offset below is
/// relative to that, and comes from `wit_parser`'s own layout code by way of
/// [`Sum::payload`].
///
/// `value` is the value as [`from_rocq`] leaves it: a scalar is already the
/// core value and is stored as it stands. A sum has no `from_rocq` -- it is
/// not one core value -- so for a sum `value` is the CertiRocq value itself,
/// and reading the payloads out of it is what [`lift_sum`] does. Converting
/// here as well would load through an `i64` that is already the payload, which
/// is what the hand-written `examples/plus1` adapter shows is wrong: it
/// dereferences at the call site and `$ok` stores.
pub fn store_canonical(repr: &Repr, dest: Base, offset: u32, value: &str) -> Vec<String> {
    match repr {
        Repr::Int63 | Repr::Bool | Repr::Enum { .. } => {
            vec![dest.store(payload_store(repr), offset, value)]
        }
        Repr::Sum(sum) => lift_sum(sum, dest, offset, value),
        Repr::Record(rec) => lift_record(rec, dest, offset, value),
    }
}

/// Write each of a record's fields where the canonical ABI reads it.
///
/// One statement group per field and no test at all: a record has one
/// constructor, so there is nothing to ask about the value -- which is the
/// whole difference from [`lift_sum`], where the tag bit decides what is
/// there to read.
///
/// Each field is *two* steps, and the first is easy to miss. The cell holds
/// the field as CertiRocq holds it, which for a boxed field is a pointer to
/// its payload -- so a `u64` field is `(i64.load (i32.load offset=4 ptr))`,
/// not one load. A field that is itself a record or a sum is left as the
/// pointer, because the conversion for those reads through it themselves.
fn lift_record(rec: &Record, dest: Base, offset: u32, value: &str) -> Vec<String> {
    // Nothing to write: a record with no fields is zero bytes wide in the
    // canonical ABI, and the immediate it arrives as carries nothing to put
    // in them.
    let mut out = Vec::new();
    for (i, field) in rec.fields.iter().enumerate() {
        let cell = Base(value).load("i32.load", rec.cell(i));
        let core = match &field.repr {
            Repr::Sum(_) | Repr::Record(_) => cell,
            scalar => from_rocq(scalar, &cell).value,
        };
        out.extend(store_canonical(
            &field.repr,
            dest,
            offset + field.offset,
            &core,
        ));
    }
    out
}

/// Write a sum's discriminant, and its payload if the case it holds has one.
///
/// The case comes from the value's own shape, which is where the tag bit earns
/// its keep: a case with no payload is an immediate and a case with one is a
/// pointer, so the bit separates them without reading memory
/// (`repr::IMMEDIATE_TAG_BIT`). It is the test the backend's own `match` makes
/// (`LambdaANF_to_Wasm.v:349-356`), and for the same reason.
///
/// With both cases boxed -- a `result<T, E>`, carried in Rocq's `sum` -- there
/// is no immediate to find, and the case is the ordinal in the block's first
/// cell instead. The tag bit still gets tested, because a value arriving as an
/// immediate would mean the backend had built a constructor this type has not
/// got, and reading a payload through it would load from the address
/// `2*ord+1`.
fn lift_sum(sum: &Sum, dest: Base, offset: u32, value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let tag = format!("(i32.and {value} (i32.const {}))", repr::IMMEDIATE_TAG_BIT);

    // Writing one case: the discriminant, then the payload where there is one.
    let write = |discriminant: u32, case: &SumCase| -> Vec<String> {
        let mut lines = vec![dest.store(
            store_op(sum.tag),
            offset,
            &format!("(i32.const {discriminant})"),
        )];
        if let Some(payload) = &case.payload {
            // Field 0 of the block, which is the payload as CertiRocq holds
            // it: a pointer for a boxed one, an immediate for the rest. So it
            // goes through the same conversion a top-level result does before
            // being stored -- unless it is another sum, which has none.
            let field = Base(value).load("i32.load", repr::boxed_field_offset(0));
            let core = match payload {
                Repr::Sum(_) | Repr::Record(_) => field,
                scalar => from_rocq(scalar, &field).value,
            };
            lines.extend(store_canonical(payload, dest, offset + sum.payload, &core));
        }
        lines
    };

    match sum.immediate_case() {
        // One case of each kind: the tag bit is the whole test.
        Some((immediate_at, immediate)) => {
            let boxed_at = 1 - immediate_at;
            out.push(format!("(if {tag}"));
            out.extend(branch("then", write(immediate_at, immediate)));
            out.extend(branch(
                "else",
                write(boxed_at, &sum.cases[boxed_at as usize]),
            ));
            out.push(")".into());
        }
        // Both boxed: the ordinal in the first cell picks the case, and an
        // immediate here is impossible rather than merely unexpected.
        None => {
            out.push(format!("(if {tag} (then unreachable))"));
            let CtorTag::Boxed(ordinal) = sum.cases[0].tag else {
                unreachable!("neither case is an immediate")
            };
            out.push(format!(
                "(if (i32.eq {} (i32.const {ordinal}))",
                Base(value).load("i32.load", repr::BOXED_ORDINAL_OFFSET)
            ));
            out.extend(branch("then", write(0, &sum.cases[0])));
            out.extend(branch("else", write(1, &sum.cases[1])));
            out.push(")".into());
        }
    }
    out
}

/// `(then ...)` or `(else ...)` around a branch's statements.
fn branch(keyword: &str, lines: Vec<String>) -> Vec<String> {
    let mut out = vec![format!("  ({keyword}")];
    out.extend(lines.into_iter().map(|line| format!("    {line}")));
    out.push("  )".into());
    out
}

/// The store a payload of this representation needs, which is its own width
/// rather than the area's.
pub fn payload_store(repr: &Repr) -> &'static str {
    match repr {
        Repr::Int63 => "i64.store",
        // A `bool` is one byte in the canonical ABI; an enum's payload is its
        // discriminant, as wide as its case count makes it; a sum's is its own
        // discriminant, which is one byte for two cases.
        Repr::Bool => store_op(wit_parser::Int::U8),
        Repr::Enum { tag, .. } => store_op(*tag),
        Repr::Sum(sum) => store_op(sum.tag),
        // Never asked: a record writes its own fields, each with the store
        // its own representation wants, so there is no single width for the
        // record as a whole.
        Repr::Record(rec) => {
            unreachable!("`{}` is stored field by field, not as one value", rec.wit)
        }
    }
}

/// The store instruction for an integer of this width.
pub fn store_op(int: wit_parser::Int) -> &'static str {
    match int {
        wit_parser::Int::U8 => "i32.store8",
        wit_parser::Int::U16 => "i32.store16",
        wit_parser::Int::U32 => "i32.store",
        wit_parser::Int::U64 => "i64.store",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wit_parser::Int;

    fn an_enum() -> Repr {
        Repr::Enum {
            rocq_type: "colour".into(),
            cases: 3,
            tag: Int::U8,
        }
    }

    fn slot(local: &str, wasm: &'static str) -> Flat {
        Flat {
            value: format!("(local.get ${local})"),
            wasm,
        }
    }

    /// Composing the pair has to give the expression back unchanged, which is
    /// the textual half of what `checks/roundtrip.py` then runs. For an
    /// immediate that is visible here; for a boxed value it is not, because
    /// the round trip goes through memory -- so that one is only a claim until
    /// the check runs it.
    #[test]
    fn the_immediates_round_trip_in_the_text() {
        for repr in [Repr::Bool, an_enum()] {
            let slots = [slot("v", "i32")];
            let there = to_rocq(&repr, &slots, &Cells::Nothing, Base("(local.get $box)"));
            let back = from_rocq(&repr, &there.value);
            assert!(there.prologue.is_empty(), "{repr:?} allocated nothing");
            assert!(back.prologue.is_empty());
            // Not textually the identity -- `(x << 1 | 1) >> 1` -- so what is
            // asserted is that the inverse wraps the forward direction rather
            // than reading some other value.
            assert_eq!(
                back.value,
                format!("(i32.shr_u {} (i32.const 1))", there.value)
            );
        }
    }

    /// A one-case enum is the pair where the inverse is *not* the inverse:
    /// the lift answers with the only discriminant the type has and never
    /// looks at the Rocq value, because the backend erases a constructor that
    /// distinguishes nothing and the value is a block rather than `2*ord+1`
    /// (TODO.md#enum-unit). The round trip is still the identity -- on 0 going
    /// out and on the immediate coming back -- which is what
    /// `checks/roundtrip.py` runs.
    #[test]
    fn a_one_case_enum_is_lifted_without_reading_it() {
        let unit = Repr::Enum {
            rocq_type: "one".into(),
            cases: 1,
            tag: Int::U8,
        };
        let there = to_rocq(
            &unit,
            &[slot("v", "i32")],
            &Cells::Nothing,
            Base("(local.get $box)"),
        );
        assert_eq!(
            there.value,
            "(i32.or (i32.shl (local.get $v) (i32.const 1)) (i32.const 1))"
        );
        let back = from_rocq(&unit, &there.value);
        assert_eq!(back.value, "(i32.const 0)");
        assert!(back.prologue.is_empty());
        // The value it is handed makes no difference, which is the point: a
        // block pointer halved is what the trap in TODO.md#enum-unit was.
        assert_eq!(
            from_rocq(&unit, "(local.get $whatever)").value,
            "(i32.const 0)"
        );
    }

    /// The boxed direction is two steps, and the address is what travels.
    #[test]
    fn a_boxed_value_travels_as_its_address() {
        let there = to_rocq(
            &Repr::Int63,
            &[slot("v", "i64")],
            &Cells::Payload(0),
            Base("(local.get $box)"),
        );
        assert_eq!(
            there.prologue,
            ["(i64.store (local.get $box) (local.get $v))"]
        );
        assert_eq!(there.value, "(local.get $box)");
        assert_eq!(
            from_rocq(&Repr::Int63, &there.value).value,
            "(i64.load (local.get $box))"
        );
    }

    /// A cell at offset zero is the base itself, and a store into it carries
    /// no `offset=`. What the hand-written adapters write, and so what this
    /// emitter has to write.
    #[test]
    fn the_first_cell_is_spelled_as_the_base() {
        let base = Base("(local.get $arg)");
        assert_eq!(base.addr(0), "(local.get $arg)");
        assert_eq!(
            base.store("i64.store", 0, "x"),
            "(i64.store (local.get $arg) x)"
        );
        assert_eq!(base.addr(8), "(i32.add (local.get $arg) (i32.const 8))");
        assert_eq!(
            base.store("i32.store", 4, "x"),
            "(i32.store offset=4 (local.get $arg) x)"
        );
    }

    /// A slot that carries either case's payload is read at the width the case
    /// wants: the join widens an i32 payload into an i64 slot, and reading it
    /// back has to narrow again.
    #[test]
    fn a_joined_slot_is_narrowed_to_the_case_that_reads_it() {
        assert_eq!(at_width(&slot("x", "i64"), "i64"), "(local.get $x)");
        assert_eq!(
            at_width(&slot("x", "i64"), "i32"),
            "(i32.wrap_i64 (local.get $x))"
        );
        assert_eq!(at_width(&slot("x", "i32"), "i32"), "(local.get $x)");
    }
}
