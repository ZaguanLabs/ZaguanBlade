//! Context Assembler
//!
//! The main component that assembles code context for AI prompts
//! by combining symbol data, file content, and related code.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::budget::{estimate_tokens, truncate_to_tokens, BudgetAllocation, TokenBudget};
use super::strategy::{ContextStrategy, StrategyConfig};
use crate::language_service::LanguageService;
use crate::tree_sitter::Symbol;

/// Assembled context ready for AI prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssembledContext {
    /// The main context text to include in the prompt
    pub context: String,
    /// Summary of what was included
    pub summary: ContextSummary,
    /// Token usage breakdown
    pub token_usage: TokenUsage,
    /// Files included in context
    pub files_included: Vec<String>,
    /// Symbols included in context
    pub symbols_included: Vec<SymbolInfo>,
}

/// Summary of assembled context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSummary {
    pub active_file: Option<String>,
    pub cursor_position: Option<(u32, u32)>,
    pub total_files: usize,
    pub total_symbols: usize,
    pub strategy_used: ContextStrategy,
}

/// Token usage breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub total: usize,
    pub budget: usize,
    pub utilization: f32,
}

/// Simplified symbol info for context summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub file: String,
}

/// Context assembler for building AI prompts
pub struct ContextAssembler {
    language_service: Arc<LanguageService>,
    budget: TokenBudget,
    strategy: ContextStrategy,
    config: StrategyConfig,
}

impl ContextAssembler {
    /// Create a new context assembler
    pub fn new(language_service: Arc<LanguageService>) -> Self {
        let strategy = ContextStrategy::default();
        Self {
            language_service,
            budget: TokenBudget::default(),
            strategy,
            config: StrategyConfig::for_strategy(strategy),
        }
    }

    /// Set token budget
    pub fn with_budget(mut self, budget: TokenBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Set assembly strategy
    pub fn with_strategy(mut self, strategy: ContextStrategy) -> Self {
        self.strategy = strategy;
        self.config = StrategyConfig::for_strategy(strategy);
        self
    }

    /// Set custom configuration
    pub fn with_config(mut self, config: StrategyConfig) -> Self {
        self.config = config;
        self.strategy = ContextStrategy::Custom;
        self
    }

    /// Assemble context for a cursor position
    pub fn assemble_for_cursor(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
        open_files: &[String],
    ) -> Result<AssembledContext, ContextError> {
        let available = self.budget.available_for_context();
        let mut allocation = BudgetAllocation::default();
        let mut context_parts: Vec<ContextPart> = Vec::new();
        let mut files_included = HashSet::new();
        let mut symbols_included: Vec<SymbolInfo> = Vec::new();

        // 1. Get active file content around cursor
        let active_content = self.get_cursor_context(file_path, line)?;
        let active_tokens = estimate_tokens(&active_content);

        if allocation.remaining(&self.budget) >= active_tokens {
            allocation.active_file = active_tokens;
            context_parts.push(ContextPart {
                content: active_content.clone(),
                priority: self.config.weights.active_file,
                source: ContextSource::ActiveFile(file_path.to_string()),
            });
            files_included.insert(file_path.to_string());
        }

        // 2. Get symbol at cursor and include definitions
        if self.config.include_definitions {
            if let Ok(Some(symbol)) = self
                .language_service
                .get_symbol_at(file_path, line, character)
            {
                symbols_included.push(SymbolInfo {
                    name: symbol.name.clone(),
                    kind: symbol.symbol_type.to_string(),
                    file: symbol.file_path.clone(),
                });

                // Try to get related definitions via search
                if let Ok(related) = self.language_service.search_symbols(&symbol.name, 5) {
                    for result in related {
                        if result.symbol.file_path != file_path {
                            let def_content = self.get_symbol_context(&result.symbol)?;
                            let def_tokens = estimate_tokens(&def_content);

                            if allocation.remaining(&self.budget) >= def_tokens {
                                allocation.definitions += def_tokens;
                                context_parts.push(ContextPart {
                                    content: def_content,
                                    priority: self.config.weights.definitions * result.score,
                                    source: ContextSource::Definition(result.symbol.name.clone()),
                                });
                                files_included.insert(result.symbol.file_path.clone());
                                symbols_included.push(SymbolInfo {
                                    name: result.symbol.name,
                                    kind: result.symbol.symbol_type.to_string(),
                                    file: result.symbol.file_path,
                                });
                            }
                        }
                    }
                }
            }
        }

        // 3. Include relevant symbols from current file
        if let Ok(file_symbols) = self.language_service.get_file_symbols(file_path) {
            for symbol in file_symbols.iter().take(10) {
                if !symbols_included.iter().any(|s| s.name == symbol.name) {
                    symbols_included.push(SymbolInfo {
                        name: symbol.name.clone(),
                        kind: symbol.symbol_type.to_string(),
                        file: symbol.file_path.clone(),
                    });
                }
            }
        }

        // 4. Include context from open files (if strategy allows)
        if self.config.max_open_files > 0 {
            let files_to_include: Vec<_> = open_files
                .iter()
                .filter(|f| *f != file_path && !files_included.contains(*f))
                .take(self.config.max_open_files)
                .collect();

            for open_file in files_to_include {
                if let Ok(symbols) = self.language_service.get_file_symbols(open_file) {
                    let summary = self.create_file_summary(open_file, &symbols);
                    let summary_tokens = estimate_tokens(&summary);

                    if allocation.remaining(&self.budget) >= summary_tokens {
                        allocation.open_files += summary_tokens;
                        context_parts.push(ContextPart {
                            content: summary,
                            priority: self.config.weights.open_files,
                            source: ContextSource::OpenFile(open_file.clone()),
                        });
                        files_included.insert(open_file.clone());
                    }
                }
            }
        }

        // Sort by priority and build final context
        context_parts.sort_by(|a, b| {
            b.priority
                .partial_cmp(&a.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let context = self.format_context(&context_parts);
        let total_tokens = estimate_tokens(&context);

        Ok(AssembledContext {
            context,
            summary: ContextSummary {
                active_file: Some(file_path.to_string()),
                cursor_position: Some((line, character)),
                total_files: files_included.len(),
                total_symbols: symbols_included.len(),
                strategy_used: self.strategy,
            },
            token_usage: TokenUsage {
                total: total_tokens,
                budget: available,
                utilization: total_tokens as f32 / available as f32,
            },
            files_included: files_included.into_iter().collect(),
            symbols_included,
        })
    }

    /// Assemble context for a general query (no specific cursor position)
    pub fn assemble_for_query(
        &self,
        query: &str,
        open_files: &[String],
    ) -> Result<AssembledContext, ContextError> {
        let available = self.budget.available_for_context();
        let mut allocation = BudgetAllocation::default();
        let mut context_parts: Vec<ContextPart> = Vec::new();
        let mut files_included = HashSet::new();
        let mut symbols_included: Vec<SymbolInfo> = Vec::new();

        // Search for relevant symbols based on query
        if let Ok(results) = self.language_service.search_symbols(query, 20) {
            for result in results {
                let symbol_content = self.get_symbol_context(&result.symbol)?;
                let tokens = estimate_tokens(&symbol_content);

                if allocation.remaining(&self.budget) >= tokens {
                    allocation.definitions += tokens;
                    context_parts.push(ContextPart {
                        content: symbol_content,
                        priority: result.score,
                        source: ContextSource::SearchResult(result.symbol.name.clone()),
                    });
                    files_included.insert(result.symbol.file_path.clone());
                    symbols_included.push(SymbolInfo {
                        name: result.symbol.name,
                        kind: result.symbol.symbol_type.to_string(),
                        file: result.symbol.file_path,
                    });
                }
            }
        }

        // Include summaries of open files
        for open_file in open_files.iter().take(self.config.max_open_files) {
            if !files_included.contains(open_file) {
                if let Ok(symbols) = self.language_service.get_file_symbols(open_file) {
                    let summary = self.create_file_summary(open_file, &symbols);
                    let summary_tokens = estimate_tokens(&summary);

                    if allocation.remaining(&self.budget) >= summary_tokens {
                        allocation.open_files += summary_tokens;
                        context_parts.push(ContextPart {
                            content: summary,
                            priority: self.config.weights.open_files,
                            source: ContextSource::OpenFile(open_file.clone()),
                        });
                        files_included.insert(open_file.clone());
                    }
                }
            }
        }

        context_parts.sort_by(|a, b| {
            b.priority
                .partial_cmp(&a.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let context = self.format_context(&context_parts);
        let total_tokens = estimate_tokens(&context);

        Ok(AssembledContext {
            context,
            summary: ContextSummary {
                active_file: None,
                cursor_position: None,
                total_files: files_included.len(),
                total_symbols: symbols_included.len(),
                strategy_used: self.strategy,
            },
            token_usage: TokenUsage {
                total: total_tokens,
                budget: available,
                utilization: total_tokens as f32 / available as f32,
            },
            files_included: files_included.into_iter().collect(),
            symbols_included,
        })
    }

    // =========================================================================
    // Helper Methods
    // =========================================================================

    fn get_cursor_context(&self, file_path: &str, line: u32) -> Result<String, ContextError> {
        // Read file and extract lines around cursor
        let full_path = Path::new(file_path);
        let content = std::fs::read_to_string(full_path)
            .or_else(|_| {
                // Try relative to workspace
                Ok::<String, std::io::Error>(String::new())
            })
            .unwrap_or_default();

        if content.is_empty() {
            return Ok(format!(
                "// File: {}\n// (content not available)",
                file_path
            ));
        }

        let lines: Vec<&str> = content.lines().collect();
        let line_idx = line as usize;
        let expansion = self.config.cursor_expansion;

        let start = line_idx.saturating_sub(expansion);
        let end = (line_idx + expansion).min(lines.len());

        let excerpt: String = lines[start..end].join("\n");

        Ok(format!(
            "// File: {}\n// Lines {}-{}\n\n{}",
            file_path,
            start + 1,
            end,
            excerpt
        ))
    }

    fn get_symbol_context(&self, symbol: &Symbol) -> Result<String, ContextError> {
        // Read file and extract symbol's range
        let full_path = Path::new(&symbol.file_path);
        let content = std::fs::read_to_string(full_path).unwrap_or_default();

        if content.is_empty() {
            return Ok(format!(
                "// {} {} in {}\n// (content not available)",
                symbol.symbol_type, symbol.name, symbol.file_path
            ));
        }

        let lines: Vec<&str> = content.lines().collect();
        let start = symbol.range.start.line as usize;
        let end = (symbol.range.end.line as usize + 1).min(lines.len());

        let excerpt: String = lines[start..end].join("\n");

        Ok(format!(
            "// {} '{}' from {}\n{}",
            symbol.symbol_type, symbol.name, symbol.file_path, excerpt
        ))
    }

    fn create_file_summary(&self, file_path: &str, symbols: &[Symbol]) -> String {
        let mut summary = format!("// File summary: {}\n// Symbols:\n", file_path);

        for symbol in symbols.iter().take(20) {
            summary.push_str(&format!(
                "//   - {} {} (lines {}-{})\n",
                symbol.symbol_type,
                symbol.name,
                symbol.range.start.line + 1,
                symbol.range.end.line + 1
            ));
        }

        if symbols.len() > 20 {
            summary.push_str(&format!(
                "//   ... and {} more symbols\n",
                symbols.len() - 20
            ));
        }

        summary
    }

    fn format_context(&self, parts: &[ContextPart]) -> String {
        let mut result = String::new();

        for part in parts {
            if !result.is_empty() {
                result.push_str("\n\n---\n\n");
            }
            result.push_str(&part.content);
        }

        // Truncate if over budget
        let max_tokens = self.budget.available_for_context();
        if estimate_tokens(&result) > max_tokens {
            truncate_to_tokens(&result, max_tokens).to_string()
        } else {
            result
        }
    }
}

/// Internal struct for context parts with priority
struct ContextPart {
    content: String,
    priority: f32,
    #[allow(dead_code)]
    source: ContextSource,
}

/// Source of a context part
#[allow(dead_code)]
enum ContextSource {
    ActiveFile(String),
    Definition(String),
    Reference(String),
    TypeDefinition(String),
    Import(String),
    OpenFile(String),
    SearchResult(String),
}

/// Context assembly errors
#[derive(Debug)]
pub enum ContextError {
    FileNotFound(String),
    SymbolNotFound(String),
    ServiceError(String),
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextError::FileNotFound(path) => write!(f, "File not found: {}", path),
            ContextError::SymbolNotFound(name) => write!(f, "Symbol not found: {}", name),
            ContextError::ServiceError(msg) => write!(f, "Service error: {}", msg),
        }
    }
}

impl std::error::Error for ContextError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_index::SymbolStore;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_assembler() -> (ContextAssembler, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("symbols.db");
        let store = Arc::new(SymbolStore::new(&db_path).unwrap());
        let service = Arc::new(LanguageService::new(temp_dir.path().to_path_buf(), store).unwrap());
        let assembler = ContextAssembler::new(service);
        (assembler, temp_dir)
    }

    #[test]
    fn test_assemble_for_cursor() {
        let (assembler, temp_dir) = create_test_assembler();

        // Create test file
        let file_path = temp_dir.path().join("test.ts");
        fs::write(
            &file_path,
            r#"
function greet(name: string): string {
    return `Hello, ${name}!`;
}

function main() {
    console.log(greet("World"));
}
        "#,
        )
        .unwrap();

        // Index the file
        let _ = assembler.language_service.index_file("test.ts");

        let result = assembler.assemble_for_cursor(file_path.to_str().unwrap(), 5, 0, &[]);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(!ctx.context.is_empty());
        assert!(ctx.token_usage.total > 0);
    }

    #[test]
    fn test_assemble_for_query() {
        let (assembler, temp_dir) = create_test_assembler();

        // Create and index test file
        fs::write(
            temp_dir.path().join("auth.ts"),
            r#"
function authenticate(token: string): boolean {
    return token.length > 0;
}

function authorize(user: User, resource: string): boolean {
    return user.permissions.includes(resource);
}
        "#,
        )
        .unwrap();

        let _ = assembler.language_service.index_file("auth.ts");

        let result = assembler.assemble_for_query("auth", &[]);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.summary.total_symbols > 0);
    }

    #[test]
    fn test_strategy_configuration() {
        let (assembler, _temp) = create_test_assembler();

        let minimal = assembler.with_strategy(ContextStrategy::Minimal);
        assert_eq!(minimal.strategy, ContextStrategy::Minimal);
        assert!(!minimal.config.include_references);

        let (assembler2, _temp2) = create_test_assembler();
        let comprehensive = assembler2.with_strategy(ContextStrategy::Comprehensive);
        assert!(comprehensive.config.include_references);
        assert!(comprehensive.config.max_open_files >= 10);
    }
}
