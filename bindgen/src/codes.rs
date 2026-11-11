//! Every refusal code this tool can print, in one place.
//!
//! A code is a promise to three readers at once: the user who sees
//! `error[E-LIST]` and searches for it, the `TODO.md` section that says what
//! lifting it would take, and the test that pins the refusal. Spelled inline
//! at the one site that raises it, a code is a string literal nothing checks
//! -- a typo reads as a new code, and a refusal moved out of the gate loses
//! its entry in the catalogue without anything noticing.
//!
//! So they are named constants and [`ALL`] is the catalogue.
//! `every_code_is_documented` walks it against `TODO.md`, which turns the
//! gate's "if you add a code here, add the matching section there" from a
//! comment into a check.
//!
//! What is deliberately *not* here is the [`Reason`](crate::support::Reason)
//! and the `TODO.md` anchor. One code can carry two verdicts: an `f32` is
//! `E-FLOAT` and out of scope, an `f64` is `E-FLOAT` and planned for phase 4,
//! and which one it is belongs to the position rather than to the code.

/// A resource, a handle, or a method on one.
pub const RESOURCE: &str = "E-RESOURCE";
/// A world importing a function.
pub const IMPORT: &str = "E-IMPORT";
/// An export that is an interface rather than a function.
pub const EXPORT_INTERFACE: &str = "E-EXPORT-INTERFACE";
/// A world with nothing to compile.
pub const NO_EXPORT: &str = "E-NO-EXPORT";
/// More than one exported function.
pub const MULTI_EXPORT: &str = "E-MULTI-EXPORT";
/// A future, a stream, or an async export.
pub const ASYNC: &str = "E-ASYNC";
/// An export taking no parameters: a value rather than a closure.
pub const ARITY_0: &str = "E-ARITY-0";
/// Parameters that flatten to more flat values than the canonical ABI passes
/// directly, so they arrive through a pointer.
pub const ARITY_FLAT: &str = "E-ARITY-FLAT";
/// An export returning nothing, so there is nowhere to report a failure.
pub const NO_RESULT: &str = "E-NO-RESULT";
/// An err half that is not the shipped error enum.
pub const ERROR_TYPE: &str = "E-ERROR-TYPE";
/// A return type with no way to report the adapter's own failures.
pub const NO_CHANNEL: &str = "E-NO-CHANNEL";
/// `u8`, `u16` or `u32`: narrower than the int the backend boxes.
pub const INT_WIDTH: &str = "E-INT-WIDTH";
/// A signed integer.
pub const SIGNED: &str = "E-SIGNED";
/// `f32` or `f64`.
pub const FLOAT: &str = "E-FLOAT";
/// `char`.
pub const CHAR: &str = "E-CHAR";
/// `string`.
pub const STRING: &str = "E-STRING";
/// An `option` or `result` with a payload in neither case, which carries no
/// more than a `bool` and has no Rocq type to be.
pub const OPTION_UNIT: &str = "E-OPTION-UNIT";
/// A record, a tuple or a variant with a payload.
pub const STRUCTURED: &str = "E-STRUCTURED";
/// A record with no fields, which the component model cannot encode.
pub const RECORD_EMPTY: &str = "E-RECORD-EMPTY";
/// A record with exactly one field, which the backend erases to the field.
pub const RECORD_UNBOXED: &str = "E-RECORD-UNBOXED";
/// `flags`.
pub const FLAGS: &str = "E-FLAGS";
/// `list`.
pub const LIST: &str = "E-LIST";
/// `map`.
pub const MAP: &str = "E-MAP";
/// A type the resolver could not resolve.
pub const UNKNOWN: &str = "E-UNKNOWN";

/// The catalogue. A code that is not in here is not documented by the test
/// that walks this list, which is the point of having the list.
pub const ALL: &[&str] = &[
    RESOURCE,
    IMPORT,
    EXPORT_INTERFACE,
    NO_EXPORT,
    MULTI_EXPORT,
    ASYNC,
    ARITY_0,
    ARITY_FLAT,
    NO_RESULT,
    ERROR_TYPE,
    NO_CHANNEL,
    INT_WIDTH,
    SIGNED,
    FLOAT,
    CHAR,
    STRING,
    OPTION_UNIT,
    STRUCTURED,
    RECORD_EMPTY,
    RECORD_UNBOXED,
    FLAGS,
    LIST,
    MAP,
    UNKNOWN,
];
