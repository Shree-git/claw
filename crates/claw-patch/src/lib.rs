//! Codec-aware diff, patch, invert, commute, and merge behavior.
//!
//! Claw VCS uses codecs so different content types can have appropriate merge
//! semantics. Built-in codecs cover line-oriented text, structural JSON,
//! TOML/YAML/spec documents, notebooks, source AST-style declarations,
//! migrations, infrastructure blocks, and binary replacement.
//!
#![deny(missing_docs)]

//! # Example
//!
//! ```rust
//! use claw_patch::text_line::TextLineCodec;
//! use claw_patch::Codec;
//!
//! let codec = TextLineCodec;
//! let old = b"first\nsecond\nthird\n";
//! let new = b"first\nchanged\nthird\n";
//!
//! let ops = codec.diff(old, new)?;
//! let applied = codec.apply(old, &ops)?;
//!
//! assert_eq!(applied, new);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
/// Binary replacement codec.
pub mod binary;
/// Codec trait shared by all patch implementations.
pub mod codec;
/// Patch codec errors.
pub mod error;
/// Structural JSON diff and merge codec.
pub mod json_tree;
/// Codec registry and path-to-codec lookup.
pub mod registry;
/// Structured document codecs built on JSON-pointer-style tree operations.
pub mod structured;
/// Line-oriented text diff and merge codec.
pub mod text_line;

/// Patch codec behavior.
pub use codec::Codec;
/// Patch codec error.
pub use error::PatchError;
/// Registry for locating codecs by id or path.
pub use registry::CodecRegistry;
