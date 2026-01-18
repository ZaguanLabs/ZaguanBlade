//! Semantic Patch Types
//!
//! Defines the structure of semantic patches that can be applied
//! to code in an AST-aware manner.

use serde::{Deserialize, Serialize};

use crate::tree_sitter::SymbolType;

/// A semantic patch describes a code modification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticPatch {
    /// Unique identifier for the patch
    pub id: String,
    /// Human-readable description
    pub description: String,
    /// Target file path
    pub file_path: String,
    /// The operation to perform
    pub operation: PatchOperation,
    /// Target specification
    pub target: PatchTarget,
    /// The new content (for insert/replace operations)
    pub content: Option<String>,
    /// Confidence score (0.0 to 1.0) for AI-generated patches
    #[serde(default = "default_confidence")]
    pub confidence: f32,
}

fn default_confidence() -> f32 {
    1.0
}

/// Types of patch operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "params")]
pub enum PatchOperation {
    /// Insert new code at a position
    Insert {
        /// Where to insert relative to target
        position: InsertPosition,
    },
    /// Replace existing code
    Replace,
    /// Delete existing code
    Delete,
    /// Rename a symbol
    Rename {
        /// New name for the symbol
        new_name: String,
    },
    /// Wrap code with new code
    Wrap {
        /// Code to insert before
        before: String,
        /// Code to insert after
        after: String,
    },
    /// Move code to a different location
    Move {
        /// Target file for the move
        target_file: String,
        /// Target position in the file
        target_position: InsertPosition,
    },
}

/// Position for insertions
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertPosition {
    /// Insert before the target
    Before,
    /// Insert after the target
    After,
    /// Insert at the start of the target (inside)
    Start,
    /// Insert at the end of the target (inside)
    End,
    /// Insert at a specific line
    AtLine(u32),
}

/// Target specification for patches
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "spec")]
pub enum PatchTarget {
    /// Target a specific symbol by name
    Symbol {
        name: String,
        symbol_type: Option<SymbolType>,
    },
    /// Target a line range
    LineRange { start: u32, end: u32 },
    /// Target based on a text pattern (regex)
    Pattern {
        regex: String,
        occurrence: Option<usize>, // Which occurrence (0 = first, None = all)
    },
    /// Target based on cursor position
    Cursor { line: u32, character: u32 },
    /// Target the entire file
    File,
}

impl SemanticPatch {
    /// Create a new symbol replacement patch
    pub fn replace_symbol(
        file_path: &str,
        symbol_name: &str,
        symbol_type: Option<SymbolType>,
        new_content: &str,
        description: &str,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            description: description.to_string(),
            file_path: file_path.to_string(),
            operation: PatchOperation::Replace,
            target: PatchTarget::Symbol {
                name: symbol_name.to_string(),
                symbol_type,
            },
            content: Some(new_content.to_string()),
            confidence: 1.0,
        }
    }

    /// Create a new insert at line patch
    pub fn insert_at_line(
        file_path: &str,
        line: u32,
        position: InsertPosition,
        content: &str,
        description: &str,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            description: description.to_string(),
            file_path: file_path.to_string(),
            operation: PatchOperation::Insert { position },
            target: PatchTarget::LineRange {
                start: line,
                end: line,
            },
            content: Some(content.to_string()),
            confidence: 1.0,
        }
    }

    /// Create a delete symbol patch
    pub fn delete_symbol(
        file_path: &str,
        symbol_name: &str,
        symbol_type: Option<SymbolType>,
        description: &str,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            description: description.to_string(),
            file_path: file_path.to_string(),
            operation: PatchOperation::Delete,
            target: PatchTarget::Symbol {
                name: symbol_name.to_string(),
                symbol_type,
            },
            content: None,
            confidence: 1.0,
        }
    }

    /// Create a rename symbol patch
    pub fn rename_symbol(
        file_path: &str,
        old_name: &str,
        new_name: &str,
        symbol_type: Option<SymbolType>,
        description: &str,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            description: description.to_string(),
            file_path: file_path.to_string(),
            operation: PatchOperation::Rename {
                new_name: new_name.to_string(),
            },
            target: PatchTarget::Symbol {
                name: old_name.to_string(),
                symbol_type,
            },
            content: None,
            confidence: 1.0,
        }
    }

    /// Create a line range replacement
    pub fn replace_lines(
        file_path: &str,
        start_line: u32,
        end_line: u32,
        new_content: &str,
        description: &str,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            description: description.to_string(),
            file_path: file_path.to_string(),
            operation: PatchOperation::Replace,
            target: PatchTarget::LineRange {
                start: start_line,
                end: end_line,
            },
            content: Some(new_content.to_string()),
            confidence: 1.0,
        }
    }

    /// Set confidence level
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }
}

/// A batch of patches to apply together
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchBatch {
    /// Unique batch identifier
    pub id: String,
    /// Description of the batch
    pub description: String,
    /// Patches in this batch (order matters)
    pub patches: Vec<SemanticPatch>,
    /// Apply all or nothing
    pub atomic: bool,
}

impl PatchBatch {
    /// Create a new patch batch
    pub fn new(description: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            description: description.to_string(),
            patches: Vec::new(),
            atomic: true,
        }
    }

    /// Add a patch to the batch
    pub fn add(mut self, patch: SemanticPatch) -> Self {
        self.patches.push(patch);
        self
    }

    /// Set whether the batch is atomic
    pub fn atomic(mut self, atomic: bool) -> Self {
        self.atomic = atomic;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_symbol() {
        let patch = SemanticPatch::replace_symbol(
            "test.ts",
            "authenticate",
            Some(SymbolType::Function),
            "function authenticate() { return true; }",
            "Simplify auth function",
        );

        assert_eq!(patch.file_path, "test.ts");
        matches!(patch.operation, PatchOperation::Replace);
        matches!(patch.target, PatchTarget::Symbol { .. });
    }

    #[test]
    fn test_patch_batch() {
        let batch = PatchBatch::new("Refactoring auth")
            .add(SemanticPatch::rename_symbol(
                "auth.ts",
                "validate",
                "validateToken",
                None,
                "Rename for clarity",
            ))
            .add(SemanticPatch::delete_symbol(
                "auth.ts",
                "deprecatedCheck",
                None,
                "Remove deprecated function",
            ));

        assert_eq!(batch.patches.len(), 2);
        assert!(batch.atomic);
    }

    #[test]
    fn test_confidence() {
        let patch = SemanticPatch::replace_symbol("test.ts", "foo", None, "bar", "test")
            .with_confidence(0.85);

        assert!((patch.confidence - 0.85).abs() < 0.001);
    }
}
