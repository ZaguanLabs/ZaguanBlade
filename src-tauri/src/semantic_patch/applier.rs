//! Patch Applier
//!
//! Applies semantic patches to source files using AST-aware modification.
//! Handles conflict detection and ensures valid state transitions.

use super::diff::generate_diff;
use super::patch::{InsertPosition, PatchOperation, PatchTarget, SemanticPatch};
use crate::language_service::LanguageService;
use crate::tree_sitter::{Symbol, SymbolType};
use regex::Regex;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ApplyFileChange {
    pub new_content: String,
    pub diff: String,
    pub file_path: String,
    pub original_content: String,
}

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
    pub additional_changes: Vec<ApplyFileChange>,
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
        let content = self
            .language_service
            .get_file_content(&patch.file_path)
            .map_err(|_| ApplyError::FileNotFound(full_path_str.to_string()))?;

        if let Some(new_content) = self.try_apply_pattern_all(patch, &content)? {
            return Ok(Self::single_result(&patch.file_path, content, new_content));
        }

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
                    let (insert_start, insert_end) = self.resolve_insert_range(
                        &patch.target,
                        &patch.file_path,
                        &full_path_str,
                        *position,
                    )?;
                    self.apply_insert(&content, insert_start, insert_end, *position, new_text)
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
            PatchOperation::Wrap { before, after } => {
                self.apply_wrap(&content, start, end, before, after)
            }
            PatchOperation::Move {
                target_file,
                target_position,
            } => {
                return self.apply_move(
                &content,
                start,
                end,
                &patch.file_path,
                target_file,
                *target_position,
            )
            }
        };

        Ok(Self::single_result(&patch.file_path, content, new_content))
    }

    fn single_result(file_path: &str, original_content: String, new_content: String) -> ApplyResult {
        let primary_change = Self::build_file_change(file_path, original_content, new_content);

        ApplyResult {
            new_content: primary_change.new_content.clone(),
            diff: primary_change.diff.clone(),
            file_path: primary_change.file_path.clone(),
            original_content: primary_change.original_content.clone(),
            additional_changes: Vec::new(),
        }
    }

    fn multi_result(primary_change: ApplyFileChange, additional_changes: Vec<ApplyFileChange>) -> ApplyResult {
        ApplyResult {
            new_content: primary_change.new_content.clone(),
            diff: primary_change.diff.clone(),
            file_path: primary_change.file_path.clone(),
            original_content: primary_change.original_content.clone(),
            additional_changes,
        }
    }

    fn build_file_change(file_path: &str, original_content: String, new_content: String) -> ApplyFileChange {
        let diff_hunks = generate_diff(&original_content, &new_content, 3);
        let diff = diff_hunks
            .iter()
            .map(|h| h.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        ApplyFileChange {
            new_content,
            diff,
            file_path: file_path.to_string(),
            original_content,
        }
    }

    fn try_apply_pattern_all(
        &self,
        patch: &SemanticPatch,
        content: &str,
    ) -> Result<Option<String>, ApplyError> {
        let regex = match &patch.target {
            PatchTarget::Pattern {
                regex,
                occurrence: None,
            } => regex,
            _ => return Ok(None),
        };

        let regex = Regex::new(regex)
            .map_err(|error| ApplyError::ContentMismatch(format!("Invalid regex: {}", error)))?;

        let new_content = match &patch.operation {
            PatchOperation::Replace => {
                let replacement = patch.content.as_ref().ok_or_else(|| {
                    ApplyError::ContentMismatch("Missing content for match".to_string())
                })?;
                regex.replace_all(content, replacement.as_str()).into_owned()
            }
            PatchOperation::Delete => regex.replace_all(content, "").into_owned(),
            _ => return Ok(None),
        };

        if new_content == content {
            return Err(ApplyError::TargetNotFound(format!(
                "Pattern did not match any occurrences: {}",
                regex.as_str()
            )));
        }

        Ok(Some(new_content))
    }

    /// Resolve target to byte range
    fn resolve_target(
        &self,
        target: &PatchTarget,
        semantic_path: &str,
        full_path: &str,
    ) -> Result<(usize, usize), ApplyError> {
        // Ensure file exists/readable
        let file_content = self
            .language_service
            .get_file_content(semantic_path)
            .map_err(|_| ApplyError::FileNotFound(full_path.to_string()))?;

        match target {
            PatchTarget::Symbol { name, symbol_type } => {
                let symbol = self.resolve_symbol_target(semantic_path, name, *symbol_type)?;
                self.get_symbol_byte_range(&symbol, semantic_path)
            }
            PatchTarget::Cursor { line, character } => {
                let symbol = self.resolve_cursor_symbol_target(semantic_path, *line, *character)?;
                self.get_symbol_byte_range(&symbol, semantic_path)
            }
            PatchTarget::Pattern { regex, occurrence } => {
                self.resolve_pattern_target(semantic_path, regex, *occurrence)
            }
            PatchTarget::LineRange { start, end } => {
                self.get_line_byte_range(semantic_path, *start, *end)
            }
            PatchTarget::File => {
                Ok((0, file_content.len()))
            }
        }
    }

    /// Calculate byte range for a symbol
    fn get_symbol_byte_range(
        &self,
        symbol: &Symbol,
        file_path: &str,
    ) -> Result<(usize, usize), ApplyError> {
        self.language_service
            .get_symbol_byte_range(symbol, file_path)
            .map_err(|error| ApplyError::TargetNotFound(error.to_string()))
    }

    fn get_line_byte_range(
        &self,
        file_path: &str,
        start_line: u32,
        end_line: u32,
    ) -> Result<(usize, usize), ApplyError> {
        self.language_service
            .get_line_byte_range(file_path, start_line, end_line)
            .map_err(|error| ApplyError::TargetNotFound(error.to_string()))
    }

    fn get_symbol_inner_byte_range(
        &self,
        symbol: &Symbol,
        file_path: &str,
    ) -> Result<(usize, usize), ApplyError> {
        self.language_service
            .get_symbol_inner_byte_range(symbol, file_path)
            .map_err(|error| ApplyError::TargetNotFound(error.to_string()))
    }

    fn resolve_insert_range(
        &self,
        target: &PatchTarget,
        semantic_path: &str,
        full_path: &str,
        position: InsertPosition,
    ) -> Result<(usize, usize), ApplyError> {
        match position {
            InsertPosition::AtLine(line) => self
                .get_line_byte_range(semantic_path, line, line)
                .map(|(start, _)| (start, start)),
            InsertPosition::Start | InsertPosition::End => match target {
                PatchTarget::Symbol { name, symbol_type } => {
                    let symbol = self.resolve_symbol_target(semantic_path, name, *symbol_type)?;
                    self.get_symbol_inner_byte_range(&symbol, semantic_path)
                }
                PatchTarget::Cursor { line, character } => {
                    let symbol = self.resolve_cursor_symbol_target(semantic_path, *line, *character)?;
                    self.get_symbol_inner_byte_range(&symbol, semantic_path)
                }
                _ => self.resolve_target(target, semantic_path, full_path),
            },
            _ => self.resolve_target(target, semantic_path, full_path),
        }
    }

    // Helper to find just the identifier part of a symbol
    fn find_identifier_range(
        &self,
        target: &PatchTarget,
        semantic_path: &str,
        full_path: &str,
    ) -> Result<(usize, usize), ApplyError> {
        match target {
            PatchTarget::Symbol { name, symbol_type } => {
                let symbol = self.resolve_symbol_target(semantic_path, name, *symbol_type)?;
                self.language_service
                    .get_symbol_identifier_byte_range(&symbol, semantic_path)
                    .map_err(|error| ApplyError::TargetNotFound(error.to_string()))
            }
            PatchTarget::Cursor { line, character } => {
                let symbol = self.resolve_cursor_symbol_target(semantic_path, *line, *character)?;
                self.language_service
                    .get_symbol_identifier_byte_range(&symbol, semantic_path)
                    .map_err(|error| ApplyError::TargetNotFound(error.to_string()))
            }
            _ => self.resolve_target(target, semantic_path, full_path),
        }
    }

    fn resolve_symbol_target(
        &self,
        semantic_path: &str,
        name: &str,
        symbol_type: Option<SymbolType>,
    ) -> Result<Symbol, ApplyError> {
        let symbols = self
            .language_service
            .get_file_symbols(semantic_path)
            .map_err(|e| ApplyError::ServiceError(e.to_string()))?;

        symbols
            .into_iter()
            .find(|symbol| symbol.name == name && symbol_type.is_none_or(|kind| symbol.symbol_type == kind))
            .ok_or_else(|| ApplyError::SymbolNotFound(name.to_string()))
    }

    fn resolve_cursor_symbol_target(
        &self,
        semantic_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Symbol, ApplyError> {
        self.language_service
            .get_symbol_at(semantic_path, line, character)
            .map_err(|error| ApplyError::ServiceError(error.to_string()))?
            .ok_or_else(|| ApplyError::TargetNotFound(format!(
                "No symbol at cursor {}:{}",
                line, character
            )))
    }

    fn resolve_pattern_target(
        &self,
        semantic_path: &str,
        regex: &str,
        occurrence: Option<usize>,
    ) -> Result<(usize, usize), ApplyError> {
        let content = self
            .language_service
            .get_file_content(semantic_path)
            .map_err(|error| ApplyError::ServiceError(error.to_string()))?;
        let regex = Regex::new(regex)
            .map_err(|error| ApplyError::ContentMismatch(format!("Invalid regex: {}", error)))?;
        let occurrence = occurrence.unwrap_or(0);

        let matched = regex
            .find_iter(&content)
            .nth(occurrence);

        matched
            .map(|matched| (matched.start(), matched.end()))
            .ok_or_else(|| {
                ApplyError::TargetNotFound(format!(
                    "Pattern did not match occurrence {}: {}",
                    occurrence, regex.as_str()
                ))
            })
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

    fn apply_wrap(&self, original: &str, start: usize, end: usize, before: &str, after: &str) -> String {
        let target = &original[start..end];
        let mut new_string =
            String::with_capacity(original.len() + before.len() + after.len());
        new_string.push_str(&original[..start]);
        new_string.push_str(before);
        new_string.push_str(target);
        new_string.push_str(after);
        new_string.push_str(&original[end..]);
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
            InsertPosition::Start => {
                let mut s = String::new();
                s.push_str(&original[..start]);
                if !original[..start].ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(insertion);
                if !insertion.ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(&original[start..]);
                s
            }
            InsertPosition::End => {
                let mut s = String::new();
                s.push_str(&original[..end]);
                if !original[..end].ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(insertion);
                if !insertion.ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(&original[end..]);
                s
            }
            InsertPosition::AtLine(_) => {
                let mut s = String::new();
                s.push_str(&original[..start]);
                s.push_str(insertion);
                if !insertion.ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(&original[start..]);
                s
            }
        }
    }

    fn apply_move(
        &self,
        original: &str,
        start: usize,
        end: usize,
        source_file: &str,
        target_file: &str,
        target_position: InsertPosition,
    ) -> Result<ApplyResult, ApplyError> {
        let InsertPosition::AtLine(target_line) = target_position else {
            return Err(ApplyError::UnsupportedOperation(
                "Move currently only supports InsertPosition::AtLine".to_string(),
            ));
        };

        let (source_end, moved, source_new_content) = self.extract_move_source(original, start, end);

        if source_file == target_file {
            let (destination, _) = self
                .get_line_byte_range(source_file, target_line, target_line)
                .map_err(|error| ApplyError::TargetNotFound(error.to_string()))?;

            if destination >= start && destination <= source_end {
                return Err(ApplyError::ContentMismatch(
                    "Move destination overlaps source range".to_string(),
                ));
            }

            let adjusted_destination = if destination > source_end {
                destination - (source_end - start)
            } else {
                destination
            };

            let mut result = String::with_capacity(original.len());
            result.push_str(&source_new_content[..adjusted_destination]);
            result.push_str(&moved);
            result.push_str(&source_new_content[adjusted_destination..]);

            return Ok(Self::single_result(source_file, original.to_string(), result));
        }

        let target_full_path = self.language_service.resolve_path(target_file);
        let target_full_path_str = target_full_path.to_string_lossy();
        let target_content = self
            .language_service
            .get_file_content(target_file)
            .map_err(|_| ApplyError::FileNotFound(target_full_path_str.to_string()))?;
        let (insert_start, insert_end) = self.resolve_insert_range(
            &PatchTarget::File,
            target_file,
            &target_full_path_str,
            target_position,
        )?;
        let target_new_content = self.apply_insert(
            &target_content,
            insert_start,
            insert_end,
            target_position,
            &moved,
        );

        let primary_change = Self::build_file_change(
            source_file,
            original.to_string(),
            source_new_content,
        );
        let target_change = Self::build_file_change(
            target_file,
            target_content,
            target_new_content,
        );

        Ok(Self::multi_result(primary_change, vec![target_change]))
    }

    fn extract_move_source(&self, original: &str, start: usize, end: usize) -> (usize, String, String) {
        let mut source_end = end;
        let source_is_line_aligned = (start == 0 || original[..start].ends_with('\n'))
            && (end == original.len() || original[end..].starts_with('\n'));
        if source_is_line_aligned && end < original.len() && original[end..].starts_with('\n') {
            source_end += 1;
        }

        let moved = original[start..source_end].to_string();
        let mut without_source = String::with_capacity(original.len() - (source_end - start));
        without_source.push_str(&original[..start]);
        without_source.push_str(&original[source_end..]);
        (source_end, moved, without_source)
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

        let patch = SemanticPatch::replace_symbol(
            "replace_test.ts",
            "test",
            None,
            "function test() { return 2; }",
            "Update return value",
        );
        let result = applier.apply(&patch).unwrap();
        assert!(result.new_content.contains("return 2"));
    }

    #[test]
    fn test_apply_rename_symbol_identifier_only() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("rename_test.ts");

        fs::write(&file_path, "function test() { return testValue; }").unwrap();
        let _ = applier.language_service.index_file("rename_test.ts");

        let patch = SemanticPatch {
            id: "rename-symbol".to_string(),
            description: "Rename function symbol".to_string(),
            file_path: "rename_test.ts".to_string(),
            operation: PatchOperation::Rename {
                new_name: "renamed".to_string(),
            },
            target: PatchTarget::Symbol {
                name: "test".to_string(),
                symbol_type: Some(SymbolType::Function),
            },
            content: None,
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert_eq!(result.new_content, "function renamed() { return testValue; }");
    }

    #[test]
    fn test_apply_replace_cursor_symbol() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("cursor_replace.ts");

        fs::write(&file_path, "function greet() { return 'hi'; }\nconst untouched = 1;\n").unwrap();
        let _ = applier.language_service.index_file("cursor_replace.ts");

        let patch = SemanticPatch {
            id: "cursor-replace".to_string(),
            description: "Replace enclosing symbol from cursor".to_string(),
            file_path: "cursor_replace.ts".to_string(),
            operation: PatchOperation::Replace,
            target: PatchTarget::Cursor {
                line: 0,
                character: 12,
            },
            content: Some("function greet() { return 'hello'; }".to_string()),
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert!(result.new_content.contains("return 'hello';"));
        assert!(result.new_content.contains("const untouched = 1;"));
    }

    #[test]
    fn test_apply_rename_cursor_symbol_identifier_only() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("cursor_rename.ts");

        fs::write(&file_path, "function greet() { return greetingValue; }\n").unwrap();
        let _ = applier.language_service.index_file("cursor_rename.ts");

        let patch = SemanticPatch {
            id: "cursor-rename".to_string(),
            description: "Rename enclosing symbol from cursor".to_string(),
            file_path: "cursor_rename.ts".to_string(),
            operation: PatchOperation::Rename {
                new_name: "welcome".to_string(),
            },
            target: PatchTarget::Cursor {
                line: 0,
                character: 12,
            },
            content: None,
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert_eq!(result.new_content, "function welcome() { return greetingValue; }\n");
    }

    #[test]
    fn test_apply_insert_start_symbol_inner_body() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("insert_start.ts");

        fs::write(&file_path, "function greet() {\n  return 'hi';\n}\n").unwrap();
        let _ = applier.language_service.index_file("insert_start.ts");

        let patch = SemanticPatch {
            id: "insert-start".to_string(),
            description: "Insert at start of symbol body".to_string(),
            file_path: "insert_start.ts".to_string(),
            operation: PatchOperation::Insert {
                position: InsertPosition::Start,
            },
            target: PatchTarget::Symbol {
                name: "greet".to_string(),
                symbol_type: Some(SymbolType::Function),
            },
            content: Some("const greeting = 'hello';".to_string()),
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert!(result.new_content.contains("const greeting = 'hello';"));
        assert!(result.new_content.contains("return 'hi';"));
    }

    #[test]
    fn test_apply_insert_end_cursor_inner_body() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("insert_end.ts");

        fs::write(&file_path, "function greet() {\n  return 'hi';\n}\nconst untouched = 1;\n").unwrap();
        let _ = applier.language_service.index_file("insert_end.ts");

        let patch = SemanticPatch {
            id: "insert-end".to_string(),
            description: "Insert at end of enclosing symbol body".to_string(),
            file_path: "insert_end.ts".to_string(),
            operation: PatchOperation::Insert {
                position: InsertPosition::End,
            },
            target: PatchTarget::Cursor {
                line: 0,
                character: 12,
            },
            content: Some("console.log(greet());".to_string()),
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert!(result.new_content.contains("console.log(greet());"));
        assert!(result.new_content.contains("const untouched = 1;"));
    }

    #[test]
    fn test_apply_replace_pattern_occurrence() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("pattern_replace.ts");

        fs::write(&file_path, "const value = 1;\nconst value = 2;\n").unwrap();
        let patch = SemanticPatch {
            id: "pattern-replace".to_string(),
            description: "Replace second regex match".to_string(),
            file_path: "pattern_replace.ts".to_string(),
            operation: PatchOperation::Replace,
            target: PatchTarget::Pattern {
                regex: "value = \\d+".to_string(),
                occurrence: Some(1),
            },
            content: Some("value = 42".to_string()),
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert_eq!(result.new_content, "const value = 1;\nconst value = 42;\n");
    }

    #[test]
    fn test_apply_delete_pattern_first_occurrence() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("pattern_delete.ts");

        fs::write(&file_path, "alpha beta gamma\nalpha beta gamma\n").unwrap();
        let patch = SemanticPatch {
            id: "pattern-delete".to_string(),
            description: "Delete first regex match".to_string(),
            file_path: "pattern_delete.ts".to_string(),
            operation: PatchOperation::Delete,
            target: PatchTarget::Pattern {
                regex: "alpha beta ".to_string(),
                occurrence: Some(0),
            },
            content: None,
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert_eq!(result.new_content, "gamma\nalpha beta gamma\n");
    }

    #[test]
    fn test_apply_replace_pattern_all_occurrences() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("pattern_replace_all.ts");

        fs::write(&file_path, "const value = 1;\nconst value = 2;\n").unwrap();
        let patch = SemanticPatch {
            id: "pattern-replace-all".to_string(),
            description: "Replace all regex matches".to_string(),
            file_path: "pattern_replace_all.ts".to_string(),
            operation: PatchOperation::Replace,
            target: PatchTarget::Pattern {
                regex: "value = \\d+".to_string(),
                occurrence: None,
            },
            content: Some("value = 99".to_string()),
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert_eq!(result.new_content, "const value = 99;\nconst value = 99;\n");
    }

    #[test]
    fn test_apply_delete_pattern_all_occurrences() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("pattern_delete_all.ts");

        fs::write(&file_path, "alpha beta gamma\nalpha beta gamma\n").unwrap();
        let patch = SemanticPatch {
            id: "pattern-delete-all".to_string(),
            description: "Delete all regex matches".to_string(),
            file_path: "pattern_delete_all.ts".to_string(),
            operation: PatchOperation::Delete,
            target: PatchTarget::Pattern {
                regex: "alpha beta ".to_string(),
                occurrence: None,
            },
            content: None,
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert_eq!(result.new_content, "gamma\ngamma\n");
    }

    #[test]
    fn test_apply_wrap_symbol_target() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("wrap_symbol.ts");

        fs::write(&file_path, "function greet() { return 'hi'; }\n").unwrap();
        let _ = applier.language_service.index_file("wrap_symbol.ts");

        let patch = SemanticPatch {
            id: "wrap-symbol".to_string(),
            description: "Wrap symbol target".to_string(),
            file_path: "wrap_symbol.ts".to_string(),
            operation: PatchOperation::Wrap {
                before: "if (enabled) {\n".to_string(),
                after: "\n}".to_string(),
            },
            target: PatchTarget::Symbol {
                name: "greet".to_string(),
                symbol_type: Some(SymbolType::Function),
            },
            content: None,
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert!(result.new_content.contains("if (enabled) {\nfunction greet() { return 'hi'; }\n}"));
    }

    #[test]
    fn test_apply_wrap_cursor_target() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("wrap_cursor.ts");

        fs::write(&file_path, "function greet() { return 'hi'; }\nconst untouched = 1;\n").unwrap();
        let _ = applier.language_service.index_file("wrap_cursor.ts");

        let patch = SemanticPatch {
            id: "wrap-cursor".to_string(),
            description: "Wrap enclosing symbol from cursor".to_string(),
            file_path: "wrap_cursor.ts".to_string(),
            operation: PatchOperation::Wrap {
                before: "track(() => {\n".to_string(),
                after: "\n});".to_string(),
            },
            target: PatchTarget::Cursor {
                line: 0,
                character: 12,
            },
            content: None,
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert!(result.new_content.contains("track(() => {\nfunction greet() { return 'hi'; }\n});"));
        assert!(result.new_content.contains("const untouched = 1;"));
    }

    #[test]
    fn test_apply_insert_at_line() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("insert_at_line.ts");

        fs::write(&file_path, "const first = 1;\nconst third = 3;\n").unwrap();

        let patch = SemanticPatch {
            id: "insert-at-line".to_string(),
            description: "Insert at explicit line".to_string(),
            file_path: "insert_at_line.ts".to_string(),
            operation: PatchOperation::Insert {
                position: InsertPosition::AtLine(2),
            },
            target: PatchTarget::File,
            content: Some("const second = 2;".to_string()),
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert_eq!(
            result.new_content,
            "const first = 1;\nconst second = 2;\nconst third = 3;\n"
        );
    }

    #[test]
    fn test_apply_move_same_file_at_line() {
        let (applier, temp_dir) = create_test_env();
        let file_path = temp_dir.path().join("move_same_file.ts");

        fs::write(
            &file_path,
            "function first() { return 1; }\nfunction second() { return 2; }\n",
        )
        .unwrap();
        let _ = applier.language_service.index_file("move_same_file.ts");

        let patch = SemanticPatch {
            id: "move-same-file".to_string(),
            description: "Move second function before first".to_string(),
            file_path: "move_same_file.ts".to_string(),
            operation: PatchOperation::Move {
                target_file: "move_same_file.ts".to_string(),
                target_position: InsertPosition::AtLine(1),
            },
            target: PatchTarget::Symbol {
                name: "second".to_string(),
                symbol_type: Some(SymbolType::Function),
            },
            content: None,
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert!(result.additional_changes.is_empty());
        assert_eq!(
            result.new_content,
            "function second() { return 2; }\nfunction first() { return 1; }\n"
        );
    }

    #[test]
    fn test_apply_move_cross_file_at_line() {
        let (applier, temp_dir) = create_test_env();
        let source_path = temp_dir.path().join("move_source.ts");
        let target_path = temp_dir.path().join("move_target.ts");

        fs::write(&source_path, "function first() { return 1; }\n").unwrap();
        fs::write(
            &target_path,
            "const before = 0;\nconst after = 1;\n",
        )
        .unwrap();
        let _ = applier.language_service.index_file("move_source.ts");
        let _ = applier.language_service.index_file("move_target.ts");

        let patch = SemanticPatch {
            id: "move-cross-file".to_string(),
            description: "Move function into another file".to_string(),
            file_path: "move_source.ts".to_string(),
            operation: PatchOperation::Move {
                target_file: "move_target.ts".to_string(),
                target_position: InsertPosition::AtLine(2),
            },
            target: PatchTarget::Symbol {
                name: "first".to_string(),
                symbol_type: Some(SymbolType::Function),
            },
            content: None,
            confidence: 1.0,
        };

        let result = applier.apply(&patch).unwrap();
        assert_eq!(result.new_content, "");
        assert_eq!(result.additional_changes.len(), 1);
        assert_eq!(result.additional_changes[0].file_path, "move_target.ts");
        assert_eq!(
            result.additional_changes[0].new_content,
            "const before = 0;\nfunction first() { return 1; }\nconst after = 1;\n"
        );
    }

    #[test]
    fn test_apply_move_cross_file_non_line_rejected() {
        let (applier, temp_dir) = create_test_env();
        let source_path = temp_dir.path().join("move_source_unsupported.ts");
        let target_path = temp_dir.path().join("move_target_unsupported.ts");

        fs::write(&source_path, "function first() { return 1; }\n").unwrap();
        fs::write(&target_path, "const second = 2;\n").unwrap();
        let _ = applier.language_service.index_file("move_source_unsupported.ts");
        let _ = applier.language_service.index_file("move_target_unsupported.ts");

        let patch = SemanticPatch {
            id: "move-cross-file-unsupported".to_string(),
            description: "Cross-file move with unsupported target position".to_string(),
            file_path: "move_source_unsupported.ts".to_string(),
            operation: PatchOperation::Move {
                target_file: "move_target_unsupported.ts".to_string(),
                target_position: InsertPosition::Before,
            },
            target: PatchTarget::Symbol {
                name: "first".to_string(),
                symbol_type: Some(SymbolType::Function),
            },
            content: None,
            confidence: 1.0,
        };

        let error = applier.apply(&patch).unwrap_err();
        assert!(error
            .to_string()
            .contains("Move currently only supports InsertPosition::AtLine"));
    }
}
