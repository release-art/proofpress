//! The Rust core of proof-pdf: a strict PDF reader and the print checks.
//!
//! The modules follow the eight tiers of `docs/notes/rust-architecture.md`, which in turn follow ISO 32000-2: each tier
//! publishes events to listeners, and the checks are listeners on the last ones.

pub mod findings;
pub mod pipeline;

pub mod checks;
mod common;
pub mod content;
pub mod document;
mod file;
mod filters;
pub mod graphics;
#[cfg(feature = "python")]
mod python;
mod syntax;
mod writer;
