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
    /// A WIT `option`, or a `result` somewhere other than the return type:
    /// two cases, each carrying at most one payload, in one of the standard
    /// library's sums (TODO.md#option-result).
    ///
    /// Boxed in a different sense than [`Repr::Int63`] is. A case with a
    /// payload is a pointer to `[ord, field]`; a case without one is an
    /// immediate. So the value is a pointer *or* an immediate depending on
    /// which case it holds, and telling them apart is the tag bit
    /// (`repr::IMMEDIATE_TAG_BIT`) -- the same test the backend's own `match`
    /// makes.
    Sum(Box<Sum>),
    /// A WIT `record`: one constructor with a field per WIT field, so a
    /// pointer to `[ord, f0, ..., fn-1]` (TODO.md#structured-types).
    ///
    /// Boxed in the same sense [`Repr::Int63`] is, and unlike [`Repr::Sum`]:
    /// a record has one constructor, so the value is always a pointer and
    /// there is no tag bit to test. What is behind each cell is another
    /// value's representation, which is why building one is recursive --
    /// every field goes down before the record pointing at it.
    Record(Box<Record>),
}

/// How CertiRocq represents one constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtorTag {
    /// Nullary: the value *is* this number.
    Immediate(u32),
    /// With a payload: a pointer to a block whose first cell holds this
    /// ordinal. Ordinals are per class, so this is not the declaration index
    /// -- see `repr::ordinal`.
    Boxed(u32),
}

/// One case of a two-case sum, in canonical-ABI discriminant order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SumCase {
    /// The WIT case name, for the comments the emitters write.
    pub wit_name: &'static str,
    /// The Rocq constructor carrying it.
    pub ctor: &'static str,
    /// What it carries, or `None` for a case with no payload at all.
    pub payload: Option<Repr>,
    pub tag: CtorTag,
}

/// A two-case WIT sum and the Rocq type carrying it.
///
/// Four WIT spellings land here, and the constructor each case maps to is a
/// table rather than a formula, because the mapping is not monotone: a
/// `result<T>`'s *first* case is the one with a payload, so it is `Some`,
/// while an `option<T>`'s first case is the empty one, so it is `None`.
///
/// | WIT | Rocq | discriminant 0 | discriminant 1 |
/// | --- | --- | --- | --- |
/// | `option<T>` | `option T` | `none` → `None` | `some` → `Some` |
/// | `result<T, E>` | `sum T E` | `ok` → `inl` | `err` → `inr` |
/// | `result<T>` | `option T` | `ok` → `Some` | `err` → `None` |
/// | `result<_, E>` | `option E` | `ok` → `None` | `err` → `Some` |
///
/// Note what the third and fourth rows mean for the user's file: the ok half
/// of a `result<T>` is written `Some x`, and the ok half of a `result<_, E>`
/// is `None`. The generated `interface.v` says so next to the signature --
/// nothing else in the pipeline would tell them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sum {
    /// How the type reads in the WIT file.
    pub wit: String,
    /// The Rocq type, as a signature writes it: `option int`, `sum int bool`.
    pub rocq_type: String,
    pub cases: [SumCase; 2],
    /// Canonical-ABI numbers, from the same `SizeAlign` the component build
    /// uses: the discriminant's width, and where the payload sits inside the
    /// value. Both are needed at every nesting depth, which is why they live
    /// on the type rather than only in [`Layout`].
    pub tag: Int,
    pub payload: u32,
    pub size: u32,
    pub align: u32,
}

impl Sum {
    /// The case a value with the tag bit set holds, if either does.
    ///
    /// At most one of the two can be nullary -- a sum whose cases were both
    /// payload-less would be an enum, and the generator refuses to carry one
    /// here -- so an immediate identifies its case outright. That is what
    /// makes lifting an `option` a single test rather than a search.
    pub fn immediate_case(&self) -> Option<(u32, &SumCase)> {
        self.cases
            .iter()
            .enumerate()
            .find(|(_, c)| matches!(c.tag, CtorTag::Immediate(_)))
            .map(|(d, c)| (d as u32, c))
    }
}

/// A WIT `record` and the Rocq `Record` carrying it.
///
/// A single constructor of arity *n*, which is what CertiRocq compiles a Rocq
/// `Record` to -- so `record span { start: u64, byte-length: u64 }` is carried
/// by a generated `Record span := mk_span { start : int ; byte_length : int }`.
/// The world names the type, so there is a name to declare, which is the split
/// the rest of the generator already makes: a named WIT type gets a
/// declaration written for it (an `enum` does, and deliberately not Rocq's
/// `bool` -- see `repr::BOOL_FALSE`), and an anonymous one is carried in a
/// Stdlib type (`option` and `sum` carry an `option` and a `result`).
///
/// A field may be a record itself, so this struct nests -- and the nesting is
/// on the Rocq side only. The canonical ABI lays a record out flat, and
/// [`Field::offset`] is the offset `wit_parser` gives the field inside its own
/// record, so the two views agree about which byte is which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// How the type reads in the WIT file.
    pub wit: String,
    /// The Rocq type, as a signature writes it: `span`, `int * int`.
    pub rocq_type: String,
    /// The constructor, which is the only one the type has.
    pub ctor: String,
    /// In WIT declaration order, which is the order both representations put
    /// them in: the canonical ABI lays the fields out in it, and the Rocq
    /// constructor takes them in it.
    pub fields: Vec<Field>,
    pub size: u32,
    pub align: u32,
}

/// One field of a record, with where the canonical ABI puts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The WIT field name, for the comments the emitters write.
    pub wit_name: String,
    /// The Rocq projection. A record's field names become top-level
    /// projections, so two records with a field of the same name would have
    /// the second shadow the first -- which is why these go through
    /// `rocq_ident` against everything else the file declares.
    pub rocq_name: String,
    pub repr: Repr,
    /// Byte offset from the start of the record, from `SizeAlign`'s own field
    /// offsets rather than from arithmetic here.
    pub offset: u32,
}

impl Record {
    /// The Rocq cell holding field *i*, which is the field after the ordinal.
    pub fn cell(&self, i: usize) -> u32 {
        repr::boxed_field_offset(i as u32)
    }
}

impl Repr {
    /// The Rocq type as it is written in a signature.
    pub fn rocq_type(&self) -> String {
        match self {
            Repr::Bool => "bool".into(),
            Repr::Int63 => "int".into(),
            Repr::Enum { rocq_type, .. } => rocq_type.clone(),
            Repr::Sum(sum) => sum.rocq_type.clone(),
            Repr::Record(rec) => rec.rocq_type.clone(),
        }
    }

    /// The same, parenthesised when it is an application, so that it can be
    /// the argument of another one: `option (option int)`, not
    /// `option option int`, which Rocq would read as a two-argument `option`.
    pub fn rocq_type_arg(&self) -> String {
        let ty = self.rocq_type();
        match ty.contains(' ') {
            true => format!("({ty})"),
            false => ty,
        }
    }

    /// Is the value a pointer into the heap rather than an immediate?
    ///
    /// This is the single fact the adapter's lift and lower turn on: an
    /// immediate is passed and returned as itself, a boxed value as the
    /// address of its payload. A [`Repr::Sum`] is neither -- it is one or the
    /// other per case -- so it answers `false` and the emitters match on it
    /// rather than asking.
    pub fn boxed(&self) -> bool {
        match self {
            Repr::Int63 => true,
            // A record with no fields has a *nullary* constructor, so it is
            // an immediate like any other -- `repr::nullary_immediate(0)`,
            // the same number `None` is. Nothing is allocated for it and
            // there is nothing to point at.
            Repr::Record(rec) => !rec.fields.is_empty(),
            _ => false,
        }
    }

    /// Can a value of this type carry a `u64` that has no `Uint63.int`?
    ///
    /// Recursive, because an `option<u64>` parameter can: the payload is the
    /// same primitive int, and its range check is the same check, emitted
    /// under the tag that makes the payload live.
    pub fn carries_int63(&self) -> bool {
        match self {
            Repr::Int63 => true,
            Repr::Bool | Repr::Enum { .. } => false,
            Repr::Sum(sum) => sum
                .cases
                .iter()
                .any(|c| c.payload.as_ref().is_some_and(Repr::carries_int63)),
            Repr::Record(rec) => rec.fields.iter().any(|f| f.repr.carries_int63()),
        }
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
    ///
    /// Empty for the two types that are not all-nullary. A [`Repr::Sum`] has
    /// a per-case table of its own ([`SumCase::tag`]), because its two cases
    /// are not in the same ordinal class and no single formula covers them.
    pub fn tags(&self) -> Vec<u32> {
        match self {
            Repr::Bool => vec![
                repr::BOOL_FALSE, // false is discriminant 0
                repr::BOOL_TRUE,  // true is discriminant 1
            ],
            Repr::Enum { cases, .. } => (0..*cases as u32).map(repr::nullary_immediate).collect(),
            Repr::Int63 | Repr::Sum(_) | Repr::Record(_) => Vec::new(),
        }
    }

    /// Does the table above reduce to `2*d+1` for every case?
    ///
    /// When it does -- which is every type carried today -- the adapter lifts
    /// and lowers with a shift. When it ever does not, the shift would quietly
    /// map the user's cases onto the wrong constructors, so the emitter must
    /// refuse rather than assume.
    /// Recursive, because a shift is emitted wherever an all-nullary type is
    /// carried -- including as the payload of a sum, where nothing else would
    /// look at it.
    pub fn tags_are_the_formula(&self) -> bool {
        let mine = self
            .tags()
            .iter()
            .enumerate()
            .all(|(d, &tag)| tag == repr::nullary_immediate(d as u32));
        mine && match self {
            Repr::Sum(sum) => sum.cases.iter().all(|c| match &c.payload {
                Some(payload) => payload.tags_are_the_formula(),
                None => true,
            }),
            Repr::Record(rec) => rec.fields.iter().all(|f| f.repr.tags_are_the_formula()),
            _ => true,
        }
    }

    /// The core wasm values the canonical ABI flattens this type into, in
    /// order.
    ///
    /// One slot for a scalar. For a sum, the discriminant and then the *join*
    /// of its two cases' slots: the cases share them, so a
    /// `result<u64, bool>` arrives as `[i32, i64]` with the `bool` riding in
    /// the i64 and read back out of its low half.
    ///
    /// This is the canonical ABI's own rule rather than a convenience, which
    /// is why [`Plan::new`] holds the concatenation of these against the
    /// flattening `wit_parser` computes for the whole signature. The two
    /// agreeing is what makes it safe to read a parameter out of a particular
    /// local.
    pub fn flatten(&self) -> Result<Vec<&'static str>> {
        match self {
            Repr::Bool | Repr::Enum { .. } => Ok(vec!["i32"]),
            Repr::Int63 => Ok(vec!["i64"]),
            Repr::Sum(sum) => {
                let mut slots = vec!["i32"];
                let mut cases = sum.cases.iter().map(|c| match &c.payload {
                    Some(repr) => repr.flatten(),
                    None => Ok(Vec::new()),
                });
                let (a, b) = (cases.next().unwrap()?, cases.next().unwrap()?);
                for i in 0..a.len().max(b.len()) {
                    slots.push(match (a.get(i), b.get(i)) {
                        (Some(x), None) | (None, Some(x)) => x,
                        (Some(x), Some(y)) => join(x, y)?,
                        (None, None) => unreachable!("i is below one of the two lengths"),
                    });
                }
                Ok(slots)
            }
            // Every field, in order. The canonical ABI flattens a record by
            // concatenating its fields' flattenings -- there is no
            // discriminant and nothing is shared, because all of the fields
            // are part of the value at once.
            Repr::Record(rec) => Ok(rec
                .fields
                .iter()
                .map(|f| f.repr.flatten())
                .collect::<Result<Vec<_>>>()?
                .concat()),
        }
    }

    /// The heap cells a *lowering* of this type writes into, as offsets from
    /// the start of the parameter's own block, and the size of the block.
    ///
    /// Nothing is allocated at run time: the adapter takes every cell it could
    /// need off the bump pointer before the program runs
    /// ([`layout`]), so the shape has to be known here. Both cases of a sum
    /// get their own cells even though only one of them is ever written --
    /// reusing one region for both would be sound today and a trap waiting for
    /// the first case that is live across the other's store.
    pub fn cells(&self, next: &mut u32) -> Cells {
        match self {
            Repr::Bool | Repr::Enum { .. } => Cells::Nothing,
            Repr::Int63 => {
                let at = *next;
                *next += repr::PRIM_PAYLOAD_SIZE;
                Cells::Payload(at)
            }
            Repr::Sum(sum) => {
                // `[ord, field]`: the block a boxed case points at. Shared
                // between the cases, since exactly one of them is built.
                let at = *next;
                *next += repr::CELL * 2;
                Cells::Block {
                    at,
                    inner: sum
                        .cases
                        .iter()
                        .map(|c| match &c.payload {
                            Some(repr) => repr.cells(next),
                            None => Cells::Nothing,
                        })
                        .collect(),
                }
            }
            // No fields, so a nullary constructor: an immediate, with
            // nothing to allocate.
            Repr::Record(rec) if rec.fields.is_empty() => Cells::Nothing,
            Repr::Record(rec) => {
                // `[ord, f0, ..., fn-1]`: the ordinal and one cell per field,
                // all of them written, and then whatever the values those
                // cells point at need.
                let at = *next;
                *next += repr::CELL * (rec.fields.len() as u32 + 1);
                Cells::Block {
                    at,
                    inner: rec.fields.iter().map(|f| f.repr.cells(next)).collect(),
                }
            }
        }
    }
}

/// Where the cells one parameter's lowering writes into sit, in the shape of
/// the value being built. Offsets are from the start of that parameter's
/// block; [`Param::block`] says where the block itself is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cells {
    /// An immediate: there is nothing to write.
    Nothing,
    /// An 8-byte primitive payload at this offset.
    Payload(u32),
    /// A constructor's block at this offset -- the ordinal, then one cell per
    /// field -- and then whatever the values those cells point at need.
    ///
    /// One variant for both of the shapes that have a block, because the
    /// offsets work the same way; what differs is how many of the cells are
    /// written. A sum reserves one field cell and `inner` is per *case*, since
    /// exactly one case is ever built. A record reserves one per field and
    /// `inner` is per *field*, since all of them are.
    Block { at: u32, inner: Vec<Cells> },
}

impl Cells {
    /// Does building this value touch memory at all?
    pub fn any(&self) -> bool {
        !matches!(self, Cells::Nothing)
    }
}

/// The canonical ABI's join of two flat slots: what one slot has to be to
/// carry either of two cases' values.
///
/// Only the pairs the supported subset can produce are here. `i32` into `i64`
/// is the widening; the float cases of the real rule cannot arise, because the
/// gate refuses `f32` and `f64` outright (`E-FLOAT`), and guessing at a pair
/// this has never seen is how a parameter ends up read out of the wrong local.
fn join(a: &'static str, b: &'static str) -> Result<&'static str> {
    Ok(match (a, b) {
        (x, y) if x == y => x,
        ("i32", "i64") | ("i64", "i32") => "i64",
        _ => bail!(
            "the canonical ABI joins the flat slots `{a}` and `{b}` of two cases into one, \
             and this generator has no rule for that pair -- see TODO.md#option-result"
        ),
    })
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

/// A `Record` `interface.v` has to declare, for a WIT `record`.
///
/// Separate from [`Inductive`] rather than a second shape of it, because the
/// two are emitted differently and ordered against each other: every
/// inductive is declared before every record, so a record with an `enum`
/// field refers to a type that is already there. A record with a *record*
/// field is ordered by construction -- the fields are planned before the
/// record around them, so the inner one is pushed first.
#[derive(Debug, Clone)]
pub struct RecordDecl {
    pub rocq_name: String,
    /// The one constructor. `mk_` and the type's name: a record's constructor
    /// has to be named something, and Rocq's own anonymous spelling (`Build_`)
    /// is a convention of the `Record` command rather than a name the user
    /// would guess.
    pub ctor: String,
    /// Projection name and Rocq type, in declaration order.
    pub fields: Vec<(String, String)>,
    /// The WIT name, for the comment that ties the two together.
    pub wit_name: String,
    /// Field name and WIT type, in the same order.
    pub wit_fields: Vec<(String, String)>,
}

impl RecordDecl {
    /// How the type reads in the WIT file. Rebuilt rather than quoted, so it
    /// says what was understood.
    pub fn wit_declaration(&self) -> String {
        format!(
            "record {} {{ {} }}",
            self.wit_name,
            self.wit_fields
                .iter()
                .map(|(name, ty)| format!("{name}: {ty}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
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
    /// Bytes taken off `mem_ptr`: the return area, then one box per boxed
    /// parameter. Every box is reserved up front and none is reused, because
    /// applying an argument captures its box *by address* and a later
    /// application reads back through that pointer (TODO.md#arity).
    pub reserve: u32,
}

/// The discriminants the adapter writes. All read off the types rather than
/// chosen, because an enum's case order *is* its discriminant.
#[derive(Debug, Clone)]
pub struct Discriminants {
    pub ok: u32,
    pub err: u32,
    pub out_of_memory: u32,
    /// `None` when no parameter can be out of range, in which case the adapter
    /// emits no range check either. One case for all of them: the enum says
    /// *that* an argument was out of range, not which.
    pub out_of_range: Option<u32>,
}

/// One core wasm parameter of the emitted function: where a flat value of the
/// canonical ABI arrives.
#[derive(Debug, Clone)]
pub struct Slot {
    /// The wat local, which is also the name in the parameter list.
    pub local: String,
    pub wasm: &'static str,
}

/// The heap cells one parameter's lowering writes into.
#[derive(Debug, Clone)]
pub struct Block {
    /// Offset of the block from the start of the adapter's reservation.
    pub offset: u32,
    /// The wat local holding its address, set once in the prologue.
    pub local: String,
    /// The cells inside it, at offsets from `local`.
    pub cells: Cells,
    pub size: u32,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub wit_name: String,
    pub rocq_name: String,
    pub repr: Repr,
    /// The core wasm parameters the canonical ABI flattens this one into, in
    /// order. One for a scalar, named after the parameter; several for a sum,
    /// which the adapter writes a comment over saying which slot is which.
    pub slots: Vec<Slot>,
    /// Where the cells this parameter's lowering needs sit, or `None` when it
    /// is built entirely out of immediates and touches no memory.
    pub block: Option<Block>,
}

impl Param {
    /// Does building this argument write into the heap?
    pub fn has_block(&self) -> bool {
        self.block.is_some()
    }

    /// The expression yielding the address of a cell at `offset` inside this
    /// parameter's block.
    ///
    /// Offset zero is the block itself, spelled as the bare local -- which is
    /// what the hand-written `examples/plus1` adapter writes for its one box,
    /// and that adapter is what this emitter is held to.
    pub fn cell(&self, offset: u32) -> String {
        let local = &self
            .block
            .as_ref()
            .expect("a parameter with no block has no cells to address")
            .local;
        match offset {
            0 => format!("(local.get ${local})"),
            n => format!("(i32.add (local.get ${local}) (i32.const {n}))"),
        }
    }
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
    /// In WIT declaration order, which is the order the adapter applies them
    /// in: the compiled closure takes one argument at a time, so parameter
    /// *i* is the *i*-th application (TODO.md#arity).
    pub params: Vec<Param>,
    pub ok: Repr,
    pub layout: Layout,
    pub discriminants: Discriminants,
    pub failures: Vec<Failure>,
}

/// File names the generated build rules tie together.
#[derive(Debug, Clone)]
pub struct Build {
    /// `--output` for `CertiRocq Compile Wasm`, so the backend writes
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
    /// Declared after every inductive, so a record with an `enum` field
    /// refers to a type already in scope.
    pub records: Vec<RecordDecl>,
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
    // What a `result` is carried in, plus the Stdlib pair and unit. The gate
    // refuses a WIT `tuple` (TODO.md#structured-types), but `prod` and its
    // projections are in scope in the generated file either way, and a record
    // field called `fst` would shadow the one a user's own code reads a pair
    // with -- which would then quietly mean the other thing.
    "sum",
    "inl",
    "inr",
    "prod",
    "pair",
    "fst",
    "snd",
    "tt",
    // Stdlib list constructors. The gate refuses a WIT `list`, but these are
    // in scope either way and a `Parameter nil` would shadow them.
    "nil",
    "cons",
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

        // One `SizeAlign` for the whole plan: it is the canonical ABI's own
        // layout code, and every offset in the generated adapter comes out of
        // it rather than out of arithmetic here.
        let mut sizes = SizeAlign::default();
        sizes.fill(resolve);

        let mut decl = Declared::default();

        let rocq_name = rocq_ident(&wit_name, "export", &mut decl.taken)?;

        ensure!(
            !func.params.is_empty(),
            "`{wit_name}` takes no parameters, which the gate should have rejected \
             ({}) -- see TODO.md#arity",
            crate::codes::ARITY_0
        );
        let param_reprs = func
            .params
            .iter()
            .map(|p| {
                repr_of(resolve, &sizes, p.ty, &mut decl)
                    .with_context(|| format!("parameter `{}` of `{wit_name}`", p.name))
            })
            .collect::<Result<Vec<_>>>()?;

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
        let ok_repr = repr_of(resolve, &sizes, ok, &mut decl)
            .with_context(|| format!("the result of `{wit_name}`"))?;

        // The adapter lifts and lowers an all-nullary type with a shift, which
        // is only the same thing as its table of immediates while the table is
        // `2*d+1`. Checked here rather than assumed in the emitter, because a
        // table that stopped reducing to the formula would not fail -- it
        // would map the user's cases onto the wrong constructors.
        for repr in param_reprs.iter().chain([&ok_repr]) {
            ensure!(
                repr.tags_are_the_formula(),
                "`{}` is carried by a type whose constructor immediates are not `2*d+1` \
                 ({:?}), and the adapter lifts and lowers them with a shift -- see \
                 TODO.md#extract-inductives",
                repr.rocq_type(),
                repr.tags()
            );
        }

        let sig = resolve.wasm_signature(AbiVariant::GuestExport, func);
        let (layout, blocks) = layout(resolve, &sizes, result, &sig, &param_reprs, ok, err)?;
        let discriminants = discriminants(resolve, err, &param_reprs)?;

        // What the gate says can go wrong has to be what the adapter can
        // report. A failure mode added there without a branch here would
        // otherwise become a trap, or worse, a plausible answer.
        let failures = support::failure_modes(resolve, func);
        check_failures(&failures, &param_reprs, &discriminants)?;

        // A parameter's name is also the wat local the adapter reads it from, so
        // it has to clear the locals the adapter declares itself, and the
        // other parameters -- two `$ret`s in one function is not a subtle
        // failure, but it is one the generator should never emit. `rocq_ident`
        // adds each name it accepts, so the parameters are checked against
        // each other as they are taken.
        let mut locals = super::adapter::LOCALS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        // How the canonical ABI flattens the whole signature, as `wit_parser`
        // computes it, against how it flattens each parameter, as the types
        // carried here say. The adapter reads parameter *i* out of particular
        // locals, so the two disagreeing would have it read one parameter's
        // bytes as another's -- with every type in sight still checking out.
        let flat = param_reprs
            .iter()
            .map(Repr::flatten)
            .collect::<Result<Vec<_>>>()?;
        let theirs = wasm_types(&sig.params, &wit_name)?;
        ensure!(
            flat.concat() == theirs,
            "`{wit_name}` flattens to {theirs:?} in the canonical ABI, but the types this \
             generator carries its {} parameter(s) in flatten to {:?}. The adapter reads \
             each parameter out of its own local, so it cannot be emitted against a \
             disagreement -- see TODO.md#option-result",
            param_reprs.len(),
            flat.concat()
        );

        let mut params = Vec::new();
        for (i, p) in func.params.iter().enumerate() {
            let rocq_name = rocq_ident(&p.name, "parameter", &mut locals)?;
            let locals_for = slot_locals(&param_reprs[i], &rocq_name);
            debug_assert_eq!(
                locals_for.len(),
                flat[i].len(),
                "one local per flat value of `{}`",
                p.name
            );
            let slots: Vec<Slot> = flat[i]
                .iter()
                .zip(locals_for)
                .map(|(wasm, local)| Slot { local, wasm })
                .collect();
            for slot in &slots {
                if slot.local == rocq_name {
                    continue;
                }
                ensure!(
                    !locals.contains(&slot.local),
                    "`{}` arrives in {} core values, and the local for one of them \
                     (`${}`) is already taken -- by another name in this world, or by a \
                     local the generated adapter declares. Rename it in the WIT file.",
                    p.name,
                    slots.len(),
                    slot.local
                );
                locals.push(slot.local.clone());
            }
            params.push(Param {
                wit_name: p.name.clone(),
                rocq_name,
                repr: param_reprs[i].clone(),
                slots,
                block: None,
            });
        }
        // The block locals are named once the parameters are, since all but
        // the one-parameter spelling are derived from them and have to clear
        // the same set.
        for (i, block) in blocks.into_iter().enumerate() {
            let Some((offset, size, cells)) = block else {
                continue;
            };
            let local = block_local(&params, i);
            ensure!(
                local == ONE_ARG_LOCAL || !locals.contains(&local),
                "parameter `{}` needs heap cells, and the local for them (`${local}`) is \
                 already taken -- by another name in this world, or by a local the \
                 generated adapter declares. Rename it in the WIT file.",
                params[i].wit_name
            );
            locals.push(local.clone());
            params[i].block = Some(Block {
                offset,
                local,
                cells,
                size,
            });
        }

        Ok(Plan {
            module_type: module_type_name(&w.name)?,
            world_name: w.name.clone(),
            export: Export {
                signature: signature(resolve, &wit_name, func),
                wit_name,
                rocq_name,
                params,
                ok: ok_repr,
                layout,
                discriminants,
                failures,
            },
            inductives: decl.inductives,
            records: decl.records,
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

/// What `interface.v` ends up declaring, and the names already spoken for.
///
/// One value threaded through [`repr_of`] rather than three parameters: a
/// type can declare an inductive, a record, or neither, and every name any of
/// them introduces competes with every other for the same namespace -- a
/// record's field becomes a top-level projection, so a field called `start`
/// and an enum case called `start` cannot both be declared.
#[derive(Default)]
struct Declared {
    inductives: Vec<Inductive>,
    records: Vec<RecordDecl>,
    taken: Vec<String>,
}

/// Which Rocq type carries this WIT type, declaring an inductive or a record
/// if there is no counterpart to reuse.
fn repr_of(resolve: &Resolve, sizes: &SizeAlign, ty: Type, decl: &mut Declared) -> Result<Repr> {
    match ty {
        Type::Bool => Ok(Repr::Bool),
        Type::U64 => Ok(Repr::Int63),
        Type::Id(id) => {
            let def = &resolve.types[id];
            match &def.kind {
                TypeDefKind::Type(inner) => repr_of(resolve, sizes, *inner, decl),
                TypeDefKind::Enum(e) => {
                    let wit_name = def
                        .name
                        .clone()
                        .context("an anonymous enum has no name to give the inductive")?;
                    let rocq_name = match decl.inductives.iter().find(|i| i.wit_name == wit_name) {
                        // Used twice in the same signature: declare it once.
                        Some(existing) => existing.rocq_name.clone(),
                        None => {
                            let rocq_name = rocq_ident(&wit_name, "type", &mut decl.taken)?;
                            let constructors = e
                                .cases
                                .iter()
                                .map(|c| rocq_ident(&c.name, "enum case", &mut decl.taken))
                                .collect::<Result<Vec<_>>>()?;
                            decl.inductives.push(Inductive {
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
                // An `option` here is a value rather than the failure channel:
                // the channel is the export's return type and is stripped
                // before this runs (`result_halves`). Both of these are the
                // same two-case shape -- see [`Sum`].
                TypeDefKind::Option(inner) => {
                    let payload = repr_of(resolve, sizes, *inner, decl)
                        .context("the payload of an `option`")?;
                    sum_of(
                        resolve,
                        sizes,
                        ty,
                        [("none", None, None), ("some", Some(*inner), Some(payload))],
                    )
                }
                TypeDefKind::Result(r) => {
                    let half = |ty: Option<Type>,
                                what: &str,
                                decl: &mut Declared|
                     -> Result<Option<Repr>> {
                        match ty {
                            Some(ty) => Ok(Some(
                                repr_of(resolve, sizes, ty, decl)
                                    .with_context(|| format!("the {what} half of a `result`"))?,
                            )),
                            None => Ok(None),
                        }
                    };
                    let ok = half(r.ok, "ok", decl)?;
                    let err = half(r.err, "err", decl)?;
                    sum_of(resolve, sizes, ty, [("ok", r.ok, ok), ("err", r.err, err)])
                }
                // A named WIT record: the world names it, so there is a
                // `Record` to declare and the fields become projections.
                TypeDefKind::Record(r) => {
                    let wit_name = def
                        .name
                        .clone()
                        .context("an anonymous record has no name to give the Rocq type")?;
                    let types: Vec<Type> = r.fields.iter().map(|f| f.ty).collect();
                    let names: Vec<String> = r.fields.iter().map(|f| f.name.clone()).collect();
                    // Used twice in one signature -- a `span -> span` is the
                    // ordinary case -- so the second mention reuses the
                    // declaration and the projection names it already took,
                    // rather than claiming them again and colliding with
                    // itself.
                    let already = decl
                        .records
                        .iter()
                        .find(|d| d.wit_name == wit_name)
                        .map(|d| (d.rocq_name.clone(), d.ctor.clone(), d.fields.clone()));
                    let projections = already.as_ref().map(|(_, _, fields)| {
                        fields.iter().map(|(name, _)| name.clone()).collect()
                    });
                    let fields = record_fields(resolve, sizes, &types, &names, projections, decl)?;
                    let (rocq_name, ctor) = match already {
                        Some((rocq_name, ctor, _)) => (rocq_name, ctor),
                        None => {
                            let rocq_name = rocq_ident(&wit_name, "type", &mut decl.taken)?;
                            let ctor = rocq_ident(
                                &format!("mk-{wit_name}"),
                                "record constructor",
                                &mut decl.taken,
                            )?;
                            decl.records.push(RecordDecl {
                                rocq_name: rocq_name.clone(),
                                ctor: ctor.clone(),
                                fields: fields
                                    .iter()
                                    .map(|f| (f.rocq_name.clone(), f.repr.rocq_type()))
                                    .collect(),
                                wit_name,
                                wit_fields: r
                                    .fields
                                    .iter()
                                    .map(|f| (f.name.clone(), show(resolve, f.ty)))
                                    .collect(),
                            });
                            (rocq_name, ctor)
                        }
                    };
                    record_of(resolve, sizes, ty, rocq_name, ctor, fields)
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

/// A two-case WIT type as the Rocq sum carrying it.
///
/// Which sum that is depends on how many of the cases have a payload, and the
/// case-to-constructor mapping with it -- the table on [`Sum`]. The three
/// shapes are not a convenience: `option` is the only Stdlib type with one
/// constructor of each kind, so it is the only one that can carry a sum where
/// exactly one case is empty, and `sum` the only one that can carry a sum
/// where neither is.
fn sum_of(
    resolve: &Resolve,
    sizes: &SizeAlign,
    ty: Type,
    cases: [(&'static str, Option<Type>, Option<Repr>); 2],
) -> Result<Repr> {
    let [(name_a, type_a, repr_a), (name_b, type_b, repr_b)] = cases;
    let case = |wit_name: &'static str, ctor: &'static str, payload, tag| SumCase {
        wit_name,
        ctor,
        payload,
        tag,
    };
    use repr::sums::{INL_ORDINAL, INR_ORDINAL, NONE_IMMEDIATE, SOME_ORDINAL};
    let (rocq_type, cases) = match (repr_a, repr_b) {
        // One payload: Rocq's `option`, whose two constructors are one of each
        // kind. Which case is `Some` is which case has the payload, so the
        // ordering of the two spellings is opposite -- an `option<T>`'s empty
        // case comes first, a `result<T>`'s comes second.
        (None, Some(b)) => (
            format!("option {}", b.rocq_type_arg()),
            [
                case(name_a, "None", None, CtorTag::Immediate(NONE_IMMEDIATE)),
                case(name_b, "Some", Some(b), CtorTag::Boxed(SOME_ORDINAL)),
            ],
        ),
        (Some(a), None) => (
            format!("option {}", a.rocq_type_arg()),
            [
                case(name_a, "Some", Some(a), CtorTag::Boxed(SOME_ORDINAL)),
                case(name_b, "None", None, CtorTag::Immediate(NONE_IMMEDIATE)),
            ],
        ),
        // Two payloads: Rocq's `sum`, whose constructors are both boxed and so
        // are ordinals 0 and 1 in declaration order. That order is the one
        // fact here that upstream could change under us, which is why
        // `checks/representation.py` measures it.
        (Some(a), Some(b)) => (
            format!("sum {} {}", a.rocq_type_arg(), b.rocq_type_arg()),
            [
                case(name_a, "inl", Some(a), CtorTag::Boxed(INL_ORDINAL)),
                case(name_b, "inr", Some(b), CtorTag::Boxed(INR_ORDINAL)),
            ],
        ),
        // Neither: two constructors that carry nothing is an enum, and the
        // Rocq type for it would have to be declared rather than reused.
        // Refused rather than carried, because `bool` is the obvious candidate
        // and it is the one inductive whose ordinals are swapped -- it would
        // exchange the two cases (`repr::BOOL_FALSE`).
        (None, None) => bail!(
            "`{}` has a payload in neither case, so there is nothing to carry and no \
             Stdlib sum to carry it in. A two-case type with no payloads is an `enum`: \
             declare one, and its cases keep their order -- see TODO.md#option-result",
            show(resolve, ty)
        ),
    };

    Ok(Repr::Sum(Box::new(Sum {
        wit: show(resolve, ty),
        rocq_type,
        cases,
        // Two cases, so one byte, by the same rule the return area's own
        // discriminant follows.
        tag: Int::U8,
        payload: sizes
            .payload_offset(Int::U8, [type_a.as_ref(), type_b.as_ref()])
            .size_wasm32() as u32,
        size: sizes.size(&ty).size_wasm32() as u32,
        align: sizes.align(&ty).align_wasm32() as u32,
    })))
}

/// Each field of a record as the generator carries it, with the canonical
/// ABI's own offset for it.
///
/// The offsets come from `SizeAlign::field_offsets`, which is the code
/// `wasm-tools component new` uses, so a field this adapter writes at offset
/// 8 is the field the component reads at offset 8.
///
/// The projections are named here because that is where they collide: a Rocq
/// `Record` declares its field names as top-level projections, so two records
/// with a field of the same name would have the second shadow the first --
/// and `start s` would then mean the other record's field while type-checking
/// perfectly.
/// `projections` is `Some` when this record has already been declared, in
/// which case its field names are the ones it was declared with rather than
/// new ones to claim.
fn record_fields(
    resolve: &Resolve,
    sizes: &SizeAlign,
    types: &[Type],
    names: &[String],
    projections: Option<Vec<String>>,
    decl: &mut Declared,
) -> Result<Vec<Field>> {
    let offsets = sizes.field_offsets(types);
    let mut fields = Vec::with_capacity(types.len());
    for (i, (offset, ty)) in offsets.into_iter().enumerate() {
        let repr = repr_of(resolve, sizes, *ty, decl)
            .with_context(|| format!("field `{}` of a record", names[i]))?;
        let rocq_name = match &projections {
            Some(declared) => declared[i].clone(),
            None => rocq_ident(&names[i], "record field", &mut decl.taken)?,
        };
        fields.push(Field {
            wit_name: names[i].clone(),
            rocq_name,
            repr,
            offset: offset.size_wasm32() as u32,
        });
    }
    Ok(fields)
}

/// A WIT record as the Rocq record carrying it.
fn record_of(
    resolve: &Resolve,
    sizes: &SizeAlign,
    ty: Type,
    rocq_type: String,
    ctor: String,
    fields: Vec<Field>,
) -> Result<Repr> {
    Ok(Repr::Record(Box::new(Record {
        wit: show(resolve, ty),
        rocq_type,
        ctor,
        fields,
        size: sizes.size(&ty).size_wasm32() as u32,
        align: sizes.align(&ty).align_wasm32() as u32,
    })))
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

/// Where one parameter's block sits in the reservation and what is in it:
/// offset, size, and the cells the lowering writes. `None` for a parameter
/// that is an immediate and touches no memory.
type Blocks = Vec<Option<(u32, u32, Cells)>>;

/// Canonical-ABI numbers for the return area, plus the bump-pointer
/// reservation the adapter takes for it and the block each parameter needs,
/// positionally.
fn layout(
    resolve: &Resolve,
    sizes: &SizeAlign,
    result: Type,
    sig: &wit_parser::abi::WasmSignature,
    params: &[Repr],
    ok: Type,
    err: Type,
) -> Result<(Layout, Blocks)> {
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
    // Above `MAX_FLAT_PARAMS` flat values the canonical ABI passes the
    // parameters through a pointer to a tuple in memory instead of one core
    // value each, which is a different adapter prologue than the one emitted
    // here.
    //
    // This is not a bound on the arity, though it was one while every carried
    // type flattened to a single value. A sum flattens to a discriminant plus
    // the join of its cases, so nine `option<u64>` parameters reach the limit
    // that seventeen `u64` ones do -- and a user told only that nine
    // parameters are "more than 16" has no way to see why. The count that
    // trips this is the one the message names.
    let flat = params
        .iter()
        .map(|p| p.flatten().map(|slots| slots.len()))
        .sum::<Result<usize>>()?;
    ensure!(
        !sig.indirect_params,
        "[{}] this export's {} parameter(s) take {flat} flat values, more \
         than the {} the canonical ABI passes directly, so they arrive through a \
         pointer rather than one core parameter each -- which the generated \
         adapter does not read. See TODO.md#arity.",
        crate::codes::ARITY_FLAT,
        params.len(),
        wit_parser::Resolve::MAX_FLAT_PARAMS
    );

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
    // One block per parameter that needs cells, in parameter order, above the
    // return area. They are all live at once: the first application captures
    // its cells by address and the last application can still be reading them,
    // so these cannot overlap or be reused (TODO.md#arity).
    //
    // A primitive int needs one 8-byte payload; a sum needs `[ord, field]` and
    // then whatever its cases' payloads need, which is why the shape comes
    // from `Repr::cells` rather than from a count here.
    let mut reserve = area;
    let mut blocks = Vec::with_capacity(params.len());
    for param in params {
        let offset = reserve;
        let mut size = 0;
        let cells = param.cells(&mut size);
        blocks.push(match cells.any() {
            true => {
                reserve += size;
                Some((offset, size, cells))
            }
            false => None,
        });
    }

    Ok((
        Layout {
            ret_size,
            ret_align,
            payload,
            ret_tag,
            err_tag: enum_tag(resolve, err)?,
            reserve,
        },
        blocks,
    ))
}

/// One wat local per flat value a parameter arrives in, named so that the
/// adapter can be read without counting.
///
/// A parameter that is one core value keeps its own name, which is what the
/// hand-written `examples/plus1` and `examples/negb` adapters spell it.
/// Beyond that the name says which part of the value the local holds, and the
/// two composite shapes differ in what there is to say:
///
/// * A record's flat values are its fields' flattenings concatenated, so each
///   local belongs to exactly one field and is named after it --
///   `$s_byte_length` rather than `$s_f1`. The hand-written
///   `examples/record` adapter is where that comes from.
/// * A sum's two cases *share* their slots, which is what the canonical ABI's
///   join means, so a slot there belongs to neither case in particular and
///   there is no field to name it after. Those are numbered, and the adapter
///   writes a comment saying which is which.
fn slot_locals(repr: &Repr, base: &str) -> Vec<String> {
    match repr {
        Repr::Record(rec) => rec
            .fields
            .iter()
            .flat_map(|f| slot_locals(&f.repr, &format!("{base}_{}", f.rocq_name)))
            .collect(),
        Repr::Sum(sum) => {
            let n = Repr::Sum(sum.clone())
                .flatten()
                .map(|slots| slots.len())
                .unwrap_or(0);
            (0..n).map(|s| format!("{base}_f{s}")).collect()
        }
        _ => vec![base.to_string()],
    }
}

/// The wat local an argument's heap cells go in.
///
/// With one parameter there is no ambiguity about which argument `$arg` is,
/// and `$arg` is what the hand-written `examples/plus1` adapter -- the
/// reference this emitter is held to -- calls it. With several, each block is
/// named after its parameter, so which application is being emitted can be
/// read off the code.
pub const ONE_ARG_LOCAL: &str = "arg";

fn block_local(params: &[Param], i: usize) -> String {
    match params.len() {
        1 => ONE_ARG_LOCAL.to_string(),
        _ => format!("{ONE_ARG_LOCAL}_{}", params[i].rocq_name),
    }
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
fn discriminants(resolve: &Resolve, err: Type, params: &[Repr]) -> Result<Discriminants> {
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
        // One case for however many parameters can be out of range: the
        // adapter checks each of them, and every check reports the same thing.
        out_of_range: match params.iter().any(Repr::carries_int63) {
            true => Some(case("out-of-range")?),
            false => None,
        },
    })
}

/// Every way the gate says this call can fail must have somewhere to go.
fn check_failures(
    failures: &[Failure],
    params: &[Repr],
    discriminants: &Discriminants,
) -> Result<()> {
    for failure in failures {
        match failure {
            // Unconditional, and `discriminants` found its case by name or
            // failed there; nothing left to check.
            Failure::OutOfMemory => {}
            Failure::OutOfRange(what) => ensure!(
                params.iter().any(Repr::carries_int63) && discriminants.out_of_range.is_some(),
                "the gate reports that this export can fail with out of range ({what}), \
                 but the adapter emitted for its parameters ({}) has no range check. \
                 Teach the adapter that case, or narrow `support::failure_modes`",
                params
                    .iter()
                    .map(|p| p.rocq_type())
                    .collect::<Vec<_>>()
                    .join(", ")
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

/// The whole signature's core parameter types, as they are written in wat.
///
/// A flat list rather than one entry per WIT parameter, because a parameter
/// does not flatten to one core value in general: a sum flattens to a
/// discriminant and its cases' slots, a record to its fields' flattenings. So
/// what the caller compares against is the *concatenation* of the per-type
/// flattenings, and the two disagreeing in length would silently map the
/// adapter's second local onto the first parameter's second field -- a
/// refusal rather than something to work out.
fn wasm_types(flat: &[WasmType], export: &str) -> Result<Vec<&'static str>> {
    flat.iter()
        .map(|ty| match ty {
            WasmType::I32 | WasmType::Pointer | WasmType::Length => Ok("i32"),
            WasmType::I64 => Ok("i64"),
            other => {
                bail!("`{export}` has a {other:?} parameter, which is not carried -- see TODO.md")
            }
        })
        .collect()
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
                // All four spellings, as WIT writes them: a missing half is
                // `_` on the left and nothing at all on the right, and a
                // `result` with neither is bare. Printing the commas anyway
                // would put `result<u64, >` in the generated comments, which
                // is not a type anyone can write.
                TypeDefKind::Result(r) => match (r.ok, r.err) {
                    (Some(ok), Some(err)) => {
                        format!("result<{}, {}>", show(resolve, ok), show(resolve, err))
                    }
                    (Some(ok), None) => format!("result<{}>", show(resolve, ok)),
                    (None, Some(err)) => format!("result<_, {}>", show(resolve, err)),
                    (None, None) => "result".into(),
                },
                TypeDefKind::Option(t) => format!("option<{}>", show(resolve, *t)),
                TypeDefKind::Tuple(t) => format!(
                    "tuple<{}>",
                    t.types
                        .iter()
                        .map(|ty| show(resolve, *ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
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
