//! Semantic Patch Engine
//!
//! Provides AST-aware code modification capabilities using tree-sitter.
//! Unlike text-based patches, semantic patches understand code structure
//! and can apply changes more intelligently.
//!
//! Features:
//! - AST-aware insertions and replacements
//! - Symbol-based targeting (modify specific functions/classes)
//! - Conflict detection and resolution
//! - Preview generation before applying changes

mod applier;
mod diff;
mod patch;

pub use applier::{ApplyError, ApplyResult, PatchApplier};
pub use diff::{generate_diff, DiffHunk};
pub use patch::{PatchOperation, PatchTarget, SemanticPatch};
