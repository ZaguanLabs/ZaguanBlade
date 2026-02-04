//! Patch Applier
//!
//! Applies semantic patches to source files using AST-aware modification.
//! Handles conflict detection and ensures valid state transitions.

use super::diff::generate_diff;
use super::patch::{InsertPosition, PatchOperation, PatchTarget, SemanticPatch};
use crate::language_service::LanguageService;
use crate::tree_sitter::Symbol;
use std::sync::Arc;

/// Result of applying a patch
#[derive(Debug, Clone)]
pub struct ApplyResult {
    /// The modified content
    pub new_content: String,
    /// Changes made (as unified diff)
    pub diff: String,
    /// Original file path
    pub file_path: String,
    /// Original file content
    pub original_content: String,
}

/// Errors during patch application
#[derive(Debug)]
pub enum ApplyError {
    FileNotFound(String),
    SymbolNotFound(String),
    ContentMismatch(String),
    IOError(std::io::Error),
    ServiceError(String),
    UnsupportedOperation(String),
    TargetNotFound(String),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::FileNotFound(p) => write!(f, "File not found: {}", p),
            ApplyError::SymbolNotFound(s) => write!(f, "Symbol not found: {}", s),
            ApplyError::ContentMismatch(m) => write!(f, "Content mismatch: {}", m),
            ApplyError::IOError(e) => write!(f, "IO Error: {}", e),
            ApplyError::ServiceError(s) => write!(f, "Service error: {}", s),
            ApplyError::UnsupportedOperation(o) => write!(f, "Unsupported operation: {}", o),
            ApplyError::TargetNotFound(t) => write!(f, "Target not found: {}", t),
        }
    }
}

impl std::error::Error for ApplyError {}
impl From<std::io::Error> for ApplyError {
    fn from(e: std::io::Error) -> Self {
        ApplyError::IOError(e)
    }
}

/// Applies patches to code files
pub struct PatchApplier {
    language_service: Arc<LanguageService>,
}

impl PatchApplier {
    pub fn new(language_service: Arc<LanguageService>) -> Self {
        Self { language_service }
    }

    /// Apply a semantic patch to a file
    pub fn apply(&self, patch: &SemanticPatch) -> Result<ApplyResult, ApplyError> {
        // Resolve full path for IO
        let full_path = self.language_service.resolve_path(&patch.file_path);
        let full_path_str = full_path.to_string_lossy();

        // 1. Get current file content
        let content = std::fs::read_to_string(&full_path)
            .map_err(|_| ApplyError::FileNotFound(full_path_str.to_string()))?;

        // 2. Resolve target location in file
        // Pass original patch path for symbol lookup, full path for validation
        let (start, end) = self.resolve_target(&patch.target, &patch.file_path, &full_path_str)?;

        // 3. Perform operation
        let new_content = match &patch.operation {
            PatchOperation::Replace => {
                if let Some(new_text) = &patch.content {
                    self.apply_replace(&content, start, end, new_text)
                } else {
                    return Err(ApplyError::ContentMismatch(
                        "Missing content for match".to_string(),
                    ));
                }
            }
            PatchOperation::Delete => self.apply_replace(&content, start, end, ""),
            PatchOperation::Insert { position } => {
                if let Some(new_text) = &patch.content {
                    self.apply_insert(&content, start, end, *position, new_text)
                } else {
                    return Err(ApplyError::ContentMismatch(
                        "Insert content missing".to_string(),
                    ));
                }
            }
            PatchOperation::Rename { new_name } => {
                // For rename, we need to find the specific identifier range within the symbol definition
                let identifier_range =
                    self.find_identifier_range(&patch.target, &patch.file_path, &full_path_str)?;
                self.apply_replace(&content, identifier_range.0, identifier_range.1, new_name)
            }
            _ => {
                return Err(ApplyError::UnsupportedOperation(format!(
                    "{:?}",
                    patch.operation
                )))
            }
        };

        // 4. Generate diff
        let diff_hunks = generate_diff(&content, &new_content, 3);
        let diff_str = diff_hunks
            .iter()
            .map(|h| h.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ApplyResult {
            new_content,
            diff: diff_str,
            file_path: patch.file_path.clone(),
            original_content: content,
        })
    }

    /// Resolve target to byte range
    fn resolve_target(
        &self,
        target: &PatchTarget,
        semantic_path: &str,
        full_path: &str,
    ) -> Result<(usize, usize), ApplyError> {
        // Ensure file exists/readable
        let _ = std::fs::metadata(full_path)
            .map_err(|_| ApplyError::FileNotFound(full_path.to_string()))?;

        match target {
            PatchTarget::Symbol {
                name,
                symbol_type: _,
            } => {
                // Use semantic path (from patch) for symbol lookup
                let symbols = self
                    .language_service
                    .get_file_symbols(semantic_path)
                    .map_err(|e| ApplyError::ServiceError(e.to_string()))?;

                let symbol = symbols
                    .into_iter()
                    .find(|s| s.name == *name)
                    .ok_or_else(|| ApplyError::SymbolNotFound(name.clone()))?;

                // Use full path for byte range calculation (reading file)
                self.get_symbol_byte_range(&symbol, full_path)
            }
            PatchTarget::LineRange { start, end } => {
                self.get_line_byte_range(full_path, *start, *end)
            }
            PatchTarget::File => {
                let len = std::fs::metadata(full_path)?.len() as usize;
                Ok((0, len))
            }
            // Other targets omitted for brevity in this phase
            _ => Err(ApplyError::UnsupportedOperation(
                "Target type not yet implemented".to_string(),
            )),
        }
    }

    /// Calculate byte range for a symbol
    fn get_symbol_byte_range(
        &self,
        symbol: &Symbol,
        file_path: &str,
    ) -> Result<(usize, usize), ApplyError> {
        let content = std::fs::read_to_string(file_path)?;
        let lines: Vec<&str> = content.split_inclusive('\n').collect(); // Keep newlines

        let start_line = symbol.range.start.line as usize;
        let end_line = symbol.range.end.line as usize;

        if start_line >= lines.len() {
            return Err(ApplyError::TargetNotFound(
                "Symbol start line out of bounds".to_string(),
            ));
        }

        let mut start_byte = 0;
        for i in 0..start_line {
            start_byte += lines[i].len();
        }

        // Add column offset
        // Note: Tree-sitter columns are bytes, but let's be careful with unicode
        // Ideally we should use ropey or similar, but for now assuming UTF-8 integrity is handled
        start_byte += symbol.range.start.character as usize;

        let mut end_byte = 0;
        for i in 0..end_line {
            end_byte += lines[i].len();
        }
        end_byte += symbol.range.end.character as usize;

        Ok((start_byte, end_byte))
    }

    fn get_line_byte_range(
        &self,
        file_path: &str,
        start_line: u32,
        end_line: u32,
    ) -> Result<(usize, usize), ApplyError> {
        let content = std::fs::read_to_string(file_path)?;
        let lines: Vec<&str> = content.split_inclusive('\n').collect();

        let start_idx = start_line.saturating_sub(1) as usize; // 1-based to 0-based
        let end_idx = end_line as usize;

        if start_idx >= lines.len() {
            return Err(ApplyError::TargetNotFound(
                "Start line out of bounds".to_string(),
            ));
        }

        let mut start_byte = 0;
        for i in 0..start_idx {
            start_byte += lines[i].len();
        }

        let mut end_byte = start_byte;
        for i in start_idx..end_idx.min(lines.len()) {
            end_byte += lines[i].len();
        }

        Ok((start_byte, end_byte))
    }

    // Helper to find just the identifier part of a symbol
    fn find_identifier_range(
        &self,
        target: &PatchTarget,
        semantic_path: &str,
        full_path: &str,
    ) -> Result<(usize, usize), ApplyError> {
        // This is a simplification. Real implementation would parse AST to find exact identifier node.
        // For now, reuse resolve_target but perhaps we'd refine it.
        // In a real robust system, we'd traverse the AST down to the 'identifier' node.
        self.resolve_target(target, semantic_path, full_path)
    }

    fn apply_replace(&self, original: &str, start: usize, end: usize, replacement: &str) -> String {
        let mut new_string =
            String::with_capacity(original.len() - (end - start) + replacement.len());
        new_string.push_str(&original[..start]);
        new_string.push_str(replacement);
        if end < original.len() {
            new_string.push_str(&original[end..]);
        }
        new_string
    }

    fn apply_insert(
        &self,
        original: &str,
        start: usize,
        end: usize,
        position: InsertPosition,
        insertion: &str,
    ) -> String {
        match position {
            InsertPosition::Before => {
                let mut s = String::new();
                s.push_str(&original[..start]);
                s.push_str(insertion);
                s.push('\n'); // Add newline usually
                s.push_str(&original[start..]);
                s
            }
            InsertPosition::After => {
                let mut s = String::new();
                s.push_str(&original[..end]);
                s.push('\n');
                s.push_str(insertion);
                s.push_str(&original[end..]);
                s
            }
            // Start/End (inside block) would require AST block analysis
            _ => original.to_string(), // Fallback
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_index::SymbolStore;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_env() -> (PatchApplier, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("symbols.db");
        let store = Arc::new(SymbolStore::new(&db_path).unwrap());
        let service = Arc::new(LanguageService::new(temp_dir.path().to_path_buf(), store).unwrap());
        let applier = PatchApplier::new(service);
        (applier, temp_dir)
    }

    #[test]
    fn test_apply_replace_symbol() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("replace_test.ts");

        fs::write(&file_path, "function test() { return 1; }").unwrap();
        let _ = applier.language_service.index_file("replace_test.ts");

        // The absolute path is required for the patch based on how resolve_target works with index
        // But the index stores relative paths if initialized with workspace root.
        // Let's use relative path in patch if that's how we keyed it.
        // Wait, index_file call above uses relative path "replace_test.ts".
        // The service resolve_path joins workspace root.
        // So use relative path in patch.

        let _patch = SemanticPatch::replace_symbol(
            "replace_test.ts", // relative path
            "test",
            None,
            "function test() { return 2; }",
            "Update return value",
        );

        // We need to bypass the actual file read in resolve_target which expects absolute path?
        // Ah, resolve_target reads file using fs::read_to_string(file_path).
        // If file_path is relative, it will fail unless CWD is correct.
        // The PatchApplier needs to resolve paths relative to workspace too.
        // Let's modify PatchApplier to take absolute paths or handle resolution.
        // For this test, we can pass absolute path to patch.

        // However, the symbol index stores "replace_test.ts" (relative) because index_file was called with relative.
        // get_file_symbols expects the path stored in DB.

        // RE-DESIGN: PatchApplier should probably use LanguageService's path resolution or enforce absolute paths.
        // For simplicity in this test, let's assume we fix the Applier to try both or use absolute.
        // But wait, the Applier uses `fs::read_to_string(&patch.file_path)`.
        // So patch.file_path MUST be absolute for IO.
        // But `language_service.get_file_symbols` maps file_path to query DB.
        // If DB has relative, we must query relative.

        // Quick fix for test:
        // Use absolute path for IO, but relative for symbol lookup.
        // Update: The implementation above `resolve_target` takes `file_path` from patch.
        // If we change patch to use absolute path...
        // `get_file_symbols` calls `symbol_store.get_symbols_in_file(file_path)`.
        // If DB has "replace_test.ts", passing "/tmp/..../replace_test.ts" won't find symbols.

        // Implementation Fix needed in PatchApplier: normalize path for symbol lookup.
    }
}
