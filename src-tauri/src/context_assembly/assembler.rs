//! Context Assembler
//!
//! The main component that assembles code context for AI prompts
//! by combining symbol data, file content, and related code.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::budget::{estimate_tokens, truncate_to_tokens, BudgetAllocation, TokenBudget};
use super::strategy::{ContextStrategy, StrategyConfig};
use crate::language_service::LanguageService;
use crate::tree_sitter::{Symbol, SymbolRelationshipType, SymbolType};

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
        let mut cursor_symbol: Option<Symbol> = None;
        let indexed_file_path = self.normalize_index_path(file_path);
        let normalized_open_files: Vec<String> = open_files
            .iter()
            .map(|path| self.normalize_index_path(path))
            .collect();

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
            files_included.insert(indexed_file_path.clone());
        }

        // 2. Get symbol at cursor and include definitions
        if self.config.include_definitions {
            if let Ok(Some(symbol)) =
                self.language_service
                    .get_symbol_at(&indexed_file_path, line, character)
            {
                cursor_symbol = Some(symbol.clone());
                symbols_included.push(SymbolInfo {
                    name: symbol.name.clone(),
                    kind: symbol.symbol_type.to_string(),
                    file: symbol.file_path.clone(),
                });

                // Try to get related definitions via search
                let mut preferred_files = vec![indexed_file_path.clone()];
                for open_file in &normalized_open_files {
                    if !preferred_files.iter().any(|path| path == open_file) {
                        preferred_files.push(open_file.clone());
                    }
                }

                if let Ok(related) = self.language_service.search_symbols_contextual(
                    &symbol.name,
                    5,
                    Some(&indexed_file_path),
                    &preferred_files,
                ) {
                    for result in related {
                        if result.symbol.file_path != indexed_file_path {
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

                if self.config.include_references {
                    for reference_name in self.resolve_reference_names(&symbol)? {
                        let resolved_symbols =
                            self.resolve_reference_symbols(&reference_name, &indexed_file_path)?;

                        if !resolved_symbols.is_empty() {
                            for resolved_symbol in resolved_symbols {
                                if symbols_included.iter().any(|included| {
                                    included.name == resolved_symbol.name
                                        && included.file == resolved_symbol.file_path
                                }) {
                                    continue;
                                }

                                let ref_content = self.get_symbol_context(&resolved_symbol)?;
                                let ref_tokens = estimate_tokens(&ref_content);

                                if allocation.remaining(&self.budget) < ref_tokens {
                                    continue;
                                }

                                allocation.definitions += ref_tokens;
                                context_parts.push(ContextPart {
                                    content: ref_content,
                                    priority: self.config.weights.references,
                                    source: ContextSource::Reference(resolved_symbol.name.clone()),
                                });
                                files_included.insert(resolved_symbol.file_path.clone());
                                symbols_included.push(SymbolInfo {
                                    name: resolved_symbol.name,
                                    kind: resolved_symbol.symbol_type.to_string(),
                                    file: resolved_symbol.file_path,
                                });
                            }
                            continue;
                        }

                        if let Ok(related) = self.language_service.search_symbols_contextual(
                            &reference_name,
                            3,
                            Some(&indexed_file_path),
                            &preferred_files,
                        ) {
                            for result in related {
                                if symbols_included.iter().any(|included| {
                                    included.name == result.symbol.name
                                        && included.file == result.symbol.file_path
                                }) {
                                    continue;
                                }

                                let ref_content = self.get_symbol_context(&result.symbol)?;
                                let ref_tokens = estimate_tokens(&ref_content);

                                if allocation.remaining(&self.budget) < ref_tokens {
                                    continue;
                                }

                                allocation.definitions += ref_tokens;
                                context_parts.push(ContextPart {
                                    content: ref_content,
                                    priority: self.config.weights.references * result.score,
                                    source: ContextSource::Reference(result.symbol.name.clone()),
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
        if let Ok(file_symbols) = self.language_service.get_file_symbols(&indexed_file_path) {
            let nearby_symbols =
                self.select_nearby_symbols(&file_symbols, cursor_symbol.as_ref(), line, 6);

            for symbol in nearby_symbols {
                if !symbols_included
                    .iter()
                    .any(|s| s.name == symbol.name && s.file == symbol.file_path)
                {
                    let symbol_content = self.get_symbol_context(symbol)?;
                    let symbol_tokens = estimate_tokens(&symbol_content);

                    if allocation.remaining(&self.budget) >= symbol_tokens {
                        allocation.definitions += symbol_tokens;
                        context_parts.push(ContextPart {
                            content: symbol_content,
                            priority: self.score_nearby_symbol(
                                symbol,
                                cursor_symbol.as_ref(),
                                line,
                            ),
                            source: ContextSource::Reference(symbol.name.clone()),
                        });
                        files_included.insert(symbol.file_path.clone());
                    }

                    symbols_included.push(SymbolInfo {
                        name: symbol.name.clone(),
                        kind: symbol.symbol_type.to_string(),
                        file: symbol.file_path.clone(),
                    });
                }
            }

            if self.config.include_imports {
                for imported_file in self.resolve_imported_files(&indexed_file_path, &file_symbols)
                {
                    if let Ok(symbols) = self.language_service.get_file_symbols(&imported_file) {
                        let summary = self.create_file_summary(&imported_file, &symbols);
                        let summary_tokens = estimate_tokens(&summary);

                        if allocation.remaining(&self.budget) >= summary_tokens {
                            allocation.open_files += summary_tokens;
                            context_parts.push(ContextPart {
                                content: summary,
                                priority: self.config.weights.imports,
                                source: ContextSource::Import(imported_file.clone()),
                            });
                            files_included.insert(imported_file);
                        }
                    }
                }
            }
        }

        // 4. Include context from open files (if strategy allows)
        if self.config.max_open_files > 0 {
            let files_to_include: Vec<_> = open_files
                .iter()
                .map(|path| self.normalize_index_path(path))
                .filter(|path| path != &indexed_file_path && !files_included.contains(path))
                .take(self.config.max_open_files)
                .collect();

            for open_file in files_to_include {
                if let Ok(symbols) = self.language_service.get_file_symbols(&open_file) {
                    let summary = self.create_file_summary(&open_file, &symbols);
                    let summary_tokens = estimate_tokens(&summary);

                    if allocation.remaining(&self.budget) >= summary_tokens {
                        allocation.open_files += summary_tokens;
                        context_parts.push(ContextPart {
                            content: summary,
                            priority: self.config.weights.open_files,
                            source: ContextSource::OpenFile(open_file.clone()),
                        });
                        files_included.insert(open_file);
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
        let normalized_open_files: Vec<String> = open_files
            .iter()
            .map(|path| self.normalize_index_path(path))
            .collect();
        let active_file = normalized_open_files.first().map(|path| path.as_str());

        // Search for relevant symbols based on query
        if let Ok(results) = self.language_service.search_symbols_contextual(
            query,
            20,
            active_file,
            &normalized_open_files,
        ) {
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
        for open_file in normalized_open_files
            .iter()
            .take(self.config.max_open_files)
        {
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
        let content = self
            .language_service
            .get_file_content(file_path)
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
        let excerpt = self
            .language_service
            .get_symbol_excerpt(symbol, &symbol.file_path)
            .unwrap_or_default();

        if excerpt.is_empty() {
            return Ok(format!(
                "// {} {} in {}\n// (content not available)",
                symbol.symbol_type, symbol.name, symbol.file_path
            ));
        }

        Ok(format!(
            "// {} '{}' from {}\n{}",
            symbol.symbol_type, symbol.name, symbol.file_path, excerpt
        ))
    }

    fn resolve_reference_names(&self, symbol: &Symbol) -> Result<Vec<String>, ContextError> {
        let stored = self
            .language_service
            .get_relationship_targets(&symbol.id, SymbolRelationshipType::Call, 6)
            .map_err(|error| ContextError::ServiceError(error.to_string()))?;

        if !stored.is_empty() {
            return Ok(stored);
        }

        self.extract_reference_names(symbol)
    }

    fn resolve_reference_symbols(
        &self,
        reference_name: &str,
        file_path: &str,
    ) -> Result<Vec<Symbol>, ContextError> {
        let mut resolved = Vec::new();
        let mut seen = HashSet::new();
        let file_symbols = self
            .language_service
            .get_file_symbols(file_path)
            .map_err(|error| ContextError::ServiceError(error.to_string()))?;

        self.collect_matching_symbols(&file_symbols, reference_name, &mut resolved, &mut seen);

        for imported_file in self.resolve_imported_files(file_path, &file_symbols) {
            let imported_symbols = self
                .language_service
                .get_file_symbols(&imported_file)
                .map_err(|error| ContextError::ServiceError(error.to_string()))?;
            self.collect_matching_symbols(
                &imported_symbols,
                reference_name,
                &mut resolved,
                &mut seen,
            );
        }

        resolved.truncate(3);
        Ok(resolved)
    }

    fn collect_matching_symbols(
        &self,
        symbols: &[Symbol],
        reference_name: &str,
        resolved: &mut Vec<Symbol>,
        seen: &mut HashSet<String>,
    ) {
        for symbol in symbols {
            if symbol.name != reference_name || symbol.symbol_type == SymbolType::Import {
                continue;
            }

            if seen.insert(symbol.id.clone()) {
                resolved.push(symbol.clone());
            }
        }
    }

    fn extract_reference_names(&self, symbol: &Symbol) -> Result<Vec<String>, ContextError> {
        let excerpt = self
            .language_service
            .get_symbol_excerpt(symbol, &symbol.file_path)
            .unwrap_or_default();

        if excerpt.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.collect_reference_candidates(&excerpt, &symbol.name))
    }

    fn collect_reference_candidates(
        &self,
        excerpt: &str,
        current_symbol_name: &str,
    ) -> Vec<String> {
        let mut references = Vec::new();
        let mut seen = HashSet::new();
        let mut window = VecDeque::new();

        for token in excerpt
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|token| !token.is_empty())
        {
            if token == current_symbol_name || token.len() < 3 {
                continue;
            }

            if token.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }

            if matches!(
                token,
                "return"
                    | "const"
                    | "let"
                    | "var"
                    | "function"
                    | "class"
                    | "import"
                    | "from"
                    | "export"
                    | "true"
                    | "false"
                    | "null"
                    | "self"
                    | "super"
                    | "crate"
                    | "pub"
                    | "use"
                    | "impl"
                    | "struct"
                    | "enum"
                    | "trait"
                    | "async"
                    | "await"
                    | "match"
                    | "else"
                    | "elif"
                    | "None"
            ) {
                continue;
            }

            if seen.insert(token.to_string()) {
                references.push(token.to_string());
                window.push_back(token.to_string());
            }

            if window.len() > 12 {
                window.pop_front();
            }
        }

        references.truncate(6);
        references
    }

    fn select_nearby_symbols<'a>(
        &self,
        symbols: &'a [Symbol],
        cursor_symbol: Option<&Symbol>,
        cursor_line: u32,
        limit: usize,
    ) -> Vec<&'a Symbol> {
        let mut ranked: Vec<&Symbol> = symbols.iter().collect();
        ranked.sort_by(|a, b| {
            self.score_nearby_symbol(b, cursor_symbol, cursor_line)
                .partial_cmp(&self.score_nearby_symbol(a, cursor_symbol, cursor_line))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        ranked
            .into_iter()
            .filter(|symbol| {
                cursor_symbol
                    .map(|current| current.id != symbol.id)
                    .unwrap_or(true)
            })
            .take(limit)
            .collect()
    }

    fn score_nearby_symbol(
        &self,
        symbol: &Symbol,
        cursor_symbol: Option<&Symbol>,
        cursor_line: u32,
    ) -> f32 {
        let distance = symbol.range.start.line.abs_diff(cursor_line) as f32;
        let mut score = (1.0 / (1.0 + distance / 40.0)).max(0.1);

        if symbol.range.start.line <= cursor_line && symbol.range.end.line >= cursor_line {
            score += 0.75;
        }

        if let Some(current) = cursor_symbol {
            if current.id == symbol.id {
                score += 2.0;
            }

            if current.parent_id.as_ref() == Some(&symbol.id)
                || symbol.parent_id.as_ref() == Some(&current.id)
            {
                score += 1.0;
            }

            if current.parent_id.is_some() && current.parent_id == symbol.parent_id {
                score += 0.55;
            }

            if current.symbol_type == symbol.symbol_type {
                score += 0.12;
            }
        }

        score
    }

    fn resolve_imported_files(&self, file_path: &str, file_symbols: &[Symbol]) -> Vec<String> {
        let mut imported_files = Vec::new();
        let mut seen = HashSet::new();

        if let Ok(stored_targets) = self.language_service.get_file_relationship_targets(
            file_path,
            SymbolRelationshipType::Import,
            12,
        ) {
            for import_target in stored_targets {
                if let Some(imported_file) = self.resolve_import_target(file_path, &import_target) {
                    if seen.insert(imported_file.clone()) {
                        imported_files.push(imported_file);
                    }
                }
            }

            if !imported_files.is_empty() {
                return imported_files;
            }
        }

        for symbol in file_symbols {
            if symbol.symbol_type != SymbolType::Import {
                continue;
            }

            if let Some(imported_file) = self.resolve_import_target(file_path, &symbol.name) {
                if seen.insert(imported_file.clone()) {
                    imported_files.push(imported_file);
                }
            }
        }

        imported_files
    }

    fn resolve_import_target(&self, file_path: &str, import_target: &str) -> Option<String> {
        if import_target.is_empty() {
            return None;
        }

        let base_file = self.language_service.resolve_path(file_path);
        let parent = base_file.parent()?;

        if import_target.starts_with('.') {
            let normalized = parent.join(import_target);
            return self.find_existing_import_candidate(&normalized);
        }

        if import_target.contains("::") {
            let crate_relative = import_target
                .trim_start_matches("crate::")
                .trim_start_matches("self::")
                .trim_start_matches("super::")
                .replace("::", "/");
            return self.find_existing_import_candidate(
                &self.language_service.resolve_path(&crate_relative),
            );
        }

        if import_target.contains('.') {
            let dotted = import_target.replace('.', "/");
            return self
                .find_existing_import_candidate(&self.language_service.resolve_path(&dotted));
        }

        None
    }

    fn find_existing_import_candidate(&self, base_path: &Path) -> Option<String> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        if base_path.extension().is_some() {
            candidates.push(base_path.to_path_buf());
        } else {
            for extension in ["ts", "tsx", "astro", "js", "jsx", "py", "rs"] {
                candidates.push(base_path.with_extension(extension));
            }

            for index_name in [
                "index.ts",
                "index.tsx",
                "index.js",
                "index.jsx",
                "mod.rs",
                "__init__.py",
            ] {
                candidates.push(base_path.join(index_name));
            }
        }

        candidates.into_iter().find_map(|candidate| {
            candidate
                .exists()
                .then(|| self.path_to_workspace_relative(&candidate))
        })
    }

    fn path_to_workspace_relative(&self, path: &Path) -> String {
        match path.strip_prefix(self.language_service.resolve_path("")) {
            Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
            Err(_) => path.to_string_lossy().replace('\\', "/"),
        }
    }

    fn normalize_index_path(&self, file_path: &str) -> String {
        let resolved = self.language_service.resolve_path(file_path);
        self.path_to_workspace_relative(&resolved)
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
    fn test_assemble_for_cursor_includes_imported_file() {
        let (assembler, temp_dir) = create_test_assembler();

        fs::write(
            temp_dir.path().join("utils.ts"),
            r#"
export function helper(): string {
    return "ok";
}
        "#,
        )
        .unwrap();

        let main_path = temp_dir.path().join("main.ts");
        fs::write(
            &main_path,
            r#"
import { helper } from "./utils";

function run(): string {
    return helper();
}
        "#,
        )
        .unwrap();

        let _ = assembler.language_service.index_file("utils.ts");
        let _ = assembler.language_service.index_file("main.ts");

        let result = assembler.assemble_for_cursor(main_path.to_str().unwrap(), 3, 0, &[]);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.files_included.iter().any(|file| file == "utils.ts"));
        assert!(ctx.context.contains("File summary: utils.ts"));
    }

    #[test]
    fn test_assemble_for_cursor_includes_referenced_symbol_context() {
        let (assembler, temp_dir) = create_test_assembler();

        fs::write(
            temp_dir.path().join("helpers.ts"),
            r#"
export function helperName(): string {
    return "helper";
}

export function formatGreeting(name: string): string {
    return `Hello, ${name}`;
}
        "#,
        )
        .unwrap();

        let main_path = temp_dir.path().join("main.ts");
        fs::write(
            &main_path,
            r#"
import { helperName, formatGreeting } from "./helpers";

function greetUser(): string {
    return formatGreeting(helperName());
}
        "#,
        )
        .unwrap();

        let _ = assembler.language_service.index_file("helpers.ts");
        let _ = assembler.language_service.index_file("main.ts");

        let result = assembler.assemble_for_cursor(main_path.to_str().unwrap(), 3, 0, &[]);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.context.contains("formatGreeting") || ctx.context.contains("helperName"));
    }

    #[test]
    fn test_assemble_for_cursor_prefers_imported_reference_symbol_context() {
        let (assembler, temp_dir) = create_test_assembler();

        fs::write(
            temp_dir.path().join("helpers.ts"),
            r#"
export function greetUser(): string {
    return "imported-greet";
}
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("other.ts"),
            r#"
export function greetUser(): string {
    return "unrelated-greet";
}
        "#,
        )
        .unwrap();

        let main_path = temp_dir.path().join("main.ts");
        fs::write(
            &main_path,
            r#"
import { greetUser } from "./helpers";

function run(): string {
    return greetUser();
}
        "#,
        )
        .unwrap();

        let _ = assembler.language_service.index_file("helpers.ts");
        let _ = assembler.language_service.index_file("other.ts");
        let _ = assembler.language_service.index_file("main.ts");

        let result = assembler.assemble_for_cursor(main_path.to_str().unwrap(), 3, 0, &[]);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.context.contains("imported-greet"));
        assert!(!ctx.context.contains("unrelated-greet"));
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
