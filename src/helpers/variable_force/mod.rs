//! Variable liveness analysis (mirrors RuboCop's VariableForce).
//!
//! Provides scope-based tracking of local variable writes and reads,
//! determining which assignments are "useless" (never read before being
//! overwritten or going out of scope).
//!
//! Used by `Lint/UselessAssignment` and can be reused by other cops like
//! `Lint/ShadowedArgument`, `Lint/UnusedBlockArgument`, etc.
//!
//! ## Module structure
//!
//! - `types` — Core data types: WriteKind, WriteInfo, ScopeInfo
//! - `analyzer` — ScopeAnalyzer: entry point and reverse-flow liveness engine
//! - `collectors` — Visit-based AST collectors for gathering variable info
//! - `suggestion` — "Did you mean?" logic using Levenshtein distance
//! - `helpers` — Utility functions (param extraction, retry detection, etc.)

mod analyzer;
mod collectors;
mod helpers;
mod suggestion;
mod types;

// Re-export the public API
pub use analyzer::ScopeAnalyzer;
pub use suggestion::levenshtein;
pub use types::{ScopeInfo, WriteInfo, WriteKind};
