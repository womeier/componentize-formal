//! The two conversions between CertiRocq's values and the canonical ABI's,
//! and the fact that they are inverses.
//!
//! An export adapter and an import wrapper need the same pair of conversions
//! in opposite orders. An export takes its parameters from the canonical ABI
//! into the backend's representation, and its result back out: [`to_rocq`]
//! then [`from_rocq`]. An import wrapper, when there is one
//! (TODO.md#imports), does exactly the reverse -- it takes the Rocq arguments
//! the compiled program handed it out into core values to pass to the host,
//! and takes the value that comes back in: [`from_rocq`] then [`to_rocq`].
//!
//! So the *order* is the caller's business and the conversion is not. Both
//! directions live here, each emitted from one place, and
//! `checks/roundtrip.py` runs every pair in both orders through `wasmtime`
//! and requires the value to come back unchanged. That is the property an
//! import wrapper will rest on, and it can be established before there is one
//! -- the gate still refuses imports (`E-IMPORT`).
//!
//! Nothing here allocates. A boxed representation needs a cell to store its
//! payload in and is given the address; where that address comes from differs
//! between the two directions -- an export reserves every box up front, off
//! the bump pointer, before the program runs (TODO.md#arity) -- and an import
//! wrapper would have to find one mid-call, which is a question about imports
//! rather than about the conversion.

use super::plan::Repr;

/// Statements to emit before the value can be read, then the expression that
/// holds it.
///
/// The prologue is empty for every conversion that is pure arithmetic. It is
/// the store into a box for the one that is not, which is why these cannot
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

/// The core wasm type a value of this representation is carried in on the
/// canonical ABI side.
pub fn core_type(repr: &Repr) -> &'static str {
    match repr {
        Repr::Int63 => "i64",
        Repr::Bool | Repr::Enum { .. } => "i32",
    }
}

/// A canonical-ABI core value, as the value CertiRocq represents it with.
///
/// `box_at` is the address a boxed representation stores its payload at, as
/// the expression that yields it -- required for [`Repr::Int63`] and unused
/// for the immediates. Passing `None` for a boxed representation is a bug in
/// the caller, not a condition to report: there is no code to emit without
/// somewhere to put the payload.
///
/// `(d << 1) | 1` is `2*ord+1` with the ordinal equal to the discriminant,
/// which holds for an `enum` because the generator writes the inductive in the
/// world's case order, and for `bool` because the pipeline's swap
/// (`CertiRocq.Plugin.Loader`: `bool => "bool" [ 1 0 ]`) happens to line its
/// ordinals up with the component model's `false = 0, true = 1`. Two reasons,
/// one shift -- and [`Repr::tags_are_the_formula`] is what keeps it honest if
/// either reason stops holding.
pub fn to_rocq(repr: &Repr, value: &str, box_at: Option<&str>) -> Conversion {
    match repr {
        // Boxed: the payload is stored, and what gets passed is its address.
        Repr::Int63 => {
            let at = box_at.expect("a boxed representation needs a box to store its payload in");
            Conversion {
                prologue: vec![format!("(i64.store {at} {value})")],
                value: at.to_string(),
            }
        }
        // `i32.ne` first: the canonical ABI lifts any non-zero byte as `true`,
        // and a 2 shifted left would name a third constructor `bool` has not
        // got.
        Repr::Bool => Conversion::pure(format!(
            "(i32.or (i32.shl (i32.ne {value} (i32.const 0)) (i32.const 1)) (i32.const 1))"
        )),
        Repr::Enum { .. } => Conversion::pure(format!(
            "(i32.or (i32.shl {value} (i32.const 1)) (i32.const 1))"
        )),
    }
}

/// A CertiRocq value, as the canonical ABI's core value.
///
/// The inverse of [`to_rocq`]: a boxed value is read back through its address,
/// and `2*ord+1` inverts to a shift. Both are total on the values the other
/// direction can produce, which is what `checks/roundtrip.py` runs.
pub fn from_rocq(repr: &Repr, value: &str) -> Conversion {
    match repr {
        Repr::Int63 => Conversion::pure(format!("(i64.load {value})")),
        Repr::Bool | Repr::Enum { .. } => {
            Conversion::pure(format!("(i32.shr_u {value} (i32.const 1))"))
        }
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

    /// Composing the pair has to give the expression back unchanged, which is
    /// the textual half of what `checks/roundtrip.py` then runs. For an
    /// immediate that is visible here; for a boxed value it is not, because
    /// the round trip goes through memory -- so that one is only a claim until
    /// the check runs it.
    #[test]
    fn the_immediates_round_trip_in_the_text() {
        for repr in [Repr::Bool, an_enum()] {
            let there = to_rocq(&repr, "(local.get $v)", None);
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

    /// The boxed direction is two steps, and the address is what travels.
    #[test]
    fn a_boxed_value_travels_as_its_address() {
        let there = to_rocq(&Repr::Int63, "(local.get $v)", Some("(local.get $box)"));
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

    #[test]
    #[should_panic(expected = "needs a box")]
    fn a_boxed_value_without_a_box_is_a_bug() {
        to_rocq(&Repr::Int63, "(local.get $v)", None);
    }
}
