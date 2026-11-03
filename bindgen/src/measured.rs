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
