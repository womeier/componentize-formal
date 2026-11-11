//! Representation facts read off a module the real CertiRocq compiled.
//!
//! **Generated.** Regenerate with
//!
//! ```text
//! nix develop --command python3 checks/representation.py --update
//! ```
//!
//! This file exists so that a change in the backend's representation breaks
//! the build rather than the output. `repr.rs` states what the generator
//! believes; this states what was measured; a `const` assertion in `repr.rs`
//! compares them, so the two cannot drift without `cargo build` failing. The
//! measurement itself is only as fresh as the last run of the check, which is
//! why `nix flake check` runs it against the pinned CertiRocq.

/// `false`, as the backend emits it for a function returning `false`.
pub const BOOL_FALSE: u32 = 1;
/// `true`, likewise. Swapped relative to Rocq's declaration order --- see
/// [`crate::repr::BOOL_FALSE`].
pub const BOOL_TRUE: u32 = 3;
/// The immediates of a three-constructor nullary inductive, in declaration
/// order, which `bool` is the exception to.
pub const NULLARY_ORDINALS: [u32; 3] = [1, 3, 5];

/// `mixed := m_first | m_boxed_a _ | m_boxed_b _ | m_last`, which is where the
/// two numbering schemes come apart: the immediates of its two *nullary*
/// constructors, in declaration order. `m_last` is declaration index 3.
pub const MIXED_NULLARY_IMMEDIATES: [u32; 2] = [1, 3];

/// The same type's two *boxed* constructors: their ordinals, in declaration
/// order. They reuse the numbers the nullary ones already have, and only the
/// tag bit separates them --- see [`crate::repr::ordinal`].
pub const MIXED_BOXED_ORDINALS: [u32; 2] = [0, 1];

/// Stdlib `option`'s `None`: the immediate a lift sees when the option is
/// empty. `Some`'s ordinal, read against `inr`'s --- the two modules differ in
/// exactly that constant. See [`crate::repr::sums`].
pub const OPTION_NONE_IMMEDIATE: u32 = 1;
pub const OPTION_SOME_ORDINAL: u32 = 0;

/// Stdlib `sum`'s `inl` and `inr`, in declaration order. Both are boxed, so
/// these are the ordinals a WIT `result`'s ok and err halves are built with,
/// and a swap upstream would exchange them.
pub const SUM_BOXED_ORDINALS: [u32; 2] = [0, 1];

/// The mask the backend's own `match` tells a pointer from an immediate with,
/// read back off a compiled `match` on an `option`.
pub const IMMEDIATE_TAG_BIT: u32 = 1;
