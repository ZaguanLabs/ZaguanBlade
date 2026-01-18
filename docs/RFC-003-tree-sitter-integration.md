# RFC-003: Tree-sitter Integration for ZaguanBlade

## Status

**Draft** - Implementation Planning Phase

## Authors

Stig-Ørjan Smelror

## Target Audience

System architects, frontend engineers, backend engineers, AI engineers

## Created

2026-01-18

---

## Executive Summary

This RFC proposes integrating tree-sitter into ZaguanBlade to enable semantic code understanding across the entire application stack. Tree-sitter will enhance:

1. **RFC-002 Context System**: Semantic code references instead of line-based
2. **Blade Protocol**: AST-aware diff application and validation
3. **Editor Features**: Intelligent folding, navigation, and highlighting
4. **AI Integration**: Better context compression and code quality validation
5. **Multi-language Support**: Unified parsing for 100+ languages

**Expected Impact**: 40-60% reduction in context token usage, 80% improvement in diff application reliability, enhanced user experience through semantic code understanding.

---

## Table of Contents

1. [Motivation](#motivation)
2. [Goals & Non-Goals](#goals--non-goals)
3. [Architecture Overview](#architecture-overview)
4. [Technical Design](#technical-design)
5. [Implementation Phases](#implementation-phases)
6. [API Specifications](#api-specifications)
7. [Data Models](#data-models)
8. [Integration Points](#integration-points)
9. [Performance Considerations](#performance-considerations)
10. [Migration Strategy](#migration-strategy)
11. [Testing Strategy](#testing-strategy)
12. [Security & Privacy](#security--privacy)
13. [Success Metrics](#success-metrics)
14. [Future Enhancements](#future-enhancements)
15. [References](#references)

---

## Motivation

### Current Limitations

**1. Line-based Code References (RFC-002)**
```json
{
  "file": "src/auth.ts",
  "lines": [10, 25],
  "context": "Authentication function"
}
```

**Problems:**
- ❌ Breaks when code moves (refactoring)
- ❌ May capture partial constructs (half a function)
- ❌ No semantic understanding (is this a function, class, or statement?)
- ❌ Inefficient context assembly (send entire line ranges)

**2. Text-based Diff Application (Blade Protocol)**
- ❌ Fails on whitespace changes
- ❌ Cannot handle code movement
- ❌ No validation of AI-generated code
- ❌ Merge conflicts on simultaneous edits

**3. Limited Language Support**
```typescript
case "toml": return []; // No TOML support
case "sh": return [];   // No shell support
```

**4. Basic Editor Features**
- ❌ No semantic folding (fold by function/class)
- ❌ No symbol navigation (jump to definition)
- ❌ No structural search (find all async functions)

### Why Tree-sitter?

**Tree-sitter is:**
- ✅ **Fast**: Incremental parsing (only re-parse changed sections)
- ✅ **Robust**: Error-tolerant (works with incomplete/invalid code)
- ✅ **Universal**: 100+ language grammars available
- ✅ **Proven**: Used by GitHub, Neovim, Atom, Zed
- ✅ **WASM-ready**: Runs efficiently in browser
- ✅ **Rust-native**: Perfect for Tauri backend

---

## Goals & Non-Goals

### Goals

**Phase 1: Foundation (Weeks 1-2)**
- ✅ Integrate tree-sitter into Rust backend
- ✅ Integrate web-tree-sitter into TypeScript frontend
- ✅ Support 10 core languages (TS, JS, Rust, Python, Go, HTML, CSS, JSON, YAML, Markdown)
- ✅ Build symbol extraction API

**Phase 2: RFC-002 Enhancement (Weeks 3-4)**
- ✅ Semantic code references (symbol-based, not line-based)
- ✅ Symbol index in SQLite (`.zblade/index/symbols.db`)
- ✅ Context assembly optimization (extract minimal semantic units)
- ✅ Stable references (track symbols across refactors)

**Phase 3: Blade Protocol Enhancement (Weeks 5-6)**
- ✅ AST-aware diff application
- ✅ Semantic patch format
- ✅ Code validation before applying changes
- ✅ Conflict detection and resolution

**Phase 4: Editor Features (Weeks 7-8)**
- ✅ Semantic code folding
- ✅ Symbol-based navigation (breadcrumbs, jump to symbol)
- ✅ Structural search
- ✅ Enhanced syntax highlighting

**Phase 5: AI Integration (Weeks 9-10)**
- ✅ Context compression using AST
- ✅ Automatic moment extraction (detect refactorings)
- ✅ Code quality metrics (complexity, coupling)
- ✅ AI code validation

### Non-Goals

- ❌ Full LSP implementation (use existing LSP servers for that)
- ❌ Type checking (use TypeScript/Rust compilers)
- ❌ Code formatting (use Prettier/rustfmt)
- ❌ Replace CodeMirror's Lezer (complement, not replace)
- ❌ Real-time collaboration (separate feature)

---

## Architecture Overview

### Dual-Stack Integration

```
┌─────────────────────────────────────────────────────────────┐
│  Frontend (TypeScript + React)                              │
│                                                              │
│  ┌────────────────────────────────────────────────────┐    │
│  │  web-tree-sitter (WASM)                            │    │
│  │  - Parse files in browser                          │    │
│  │  - Real-time AST updates                           │    │
│  │  - Editor integration (CodeMirror)                 │    │
│  └────────────────────────────────────────────────────┘    │
│                          ▲                                   │
│                          │ Tauri IPC                         │
│                          ▼                                   │
│  ┌────────────────────────────────────────────────────┐    │
│  │  Rust Backend (tree-sitter crate)                  │    │
│  │  - Parse files on disk                             │    │
│  │  - Symbol extraction                               │    │
│  │  - Index management (SQLite)                       │    │
│  │  - Semantic diff application                       │    │
│  └────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
                          ▲
                          │ WebSocket
                          ▼
┌─────────────────────────────────────────────────────────────┐
│  zcoderd (Go Server)                                        │
│  - Receives semantic context (not raw code)                 │
│  - AI model integration                                     │
│  - Generates semantic patches                               │
└─────────────────────────────────────────────────────────────┘
```

### Why Dual-Stack?

**Frontend (web-tree-sitter):**
- Real-time parsing as user types
- Immediate feedback (syntax errors, folding)
- No network latency
- Privacy-preserving (local parsing)

**Backend (tree-sitter Rust):**
- Batch processing (index entire workspace)
- Cross-file analysis
- Persistent storage (symbol index)
- Heavy operations (structural search)

---

## Technical Design

### 1. Symbol Extraction System

#### Symbol Definition

```rust
// src-tauri/src/tree_sitter/symbol.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Unique identifier (file_path:symbol_name:start_byte)
    pub id: String,
    
    /// Symbol name (function/class/variable name)
    pub name: String,
    
    /// Symbol type
    pub symbol_type: SymbolType,
    
    /// File path (relative to workspace root)
    pub file_path: String,
    
    /// Byte range in file
    pub byte_range: (usize, usize),
    
    /// Line range (1-indexed)
    pub line_range: (usize, usize),
    
    /// Parent symbol (for methods in classes)
    pub parent_id: Option<String>,
    
    /// Child symbols (for classes containing methods)
    pub children: Vec<String>,
    
    /// Dependencies (imports, function calls)
    pub dependencies: Vec<Dependency>,
    
    /// Documentation string
    pub docstring: Option<String>,
    
    /// Metadata
    pub metadata: SymbolMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SymbolType {
    Function,
    Method,
    Class,
    Interface,
    Struct,
    Enum,
    Variable,
    Constant,
    Type,
    Module,
    Namespace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub dep_type: DependencyType,
    pub target: String,
    pub source_location: (usize, usize), // byte range
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DependencyType {
    Import,
    FunctionCall,
    TypeReference,
    Inheritance,
    Implementation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolMetadata {
    /// Cyclomatic complexity (for functions)
    pub complexity: Option<u32>,
    
    /// Is async function?
    pub is_async: bool,
    
    /// Is exported?
    pub is_exported: bool,
    
    /// Visibility (public, private, protected)
    pub visibility: Visibility,
    
    /// Language-specific attributes
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
}
```

#### Symbol Extractor

```rust
// src-tauri/src/tree_sitter/extractor.rs

pub struct SymbolExtractor {
    parser: Parser,
    language_config: LanguageConfig,
}

impl SymbolExtractor {
    pub fn new(language: Language) -> Result<Self, Error> {
        let mut parser = Parser::new();
        parser.set_language(language)?;
        
        Ok(Self {
            parser,
            language_config: LanguageConfig::for_language(language),
        })
    }
    
    pub fn extract_symbols(&mut self, source: &str, file_path: &str) 
        -> Result<Vec<Symbol>, Error> 
    {
        let tree = self.parser.parse(source, None)
            .ok_or(Error::ParseFailed)?;
        
        let mut symbols = Vec::new();
        let mut cursor = tree.walk();
        
        self.extract_symbols_recursive(
            &mut cursor,
            source,
            file_path,
            None,
            &mut symbols
        );
        
        Ok(symbols)
    }
    
    fn extract_symbols_recursive(
        &self,
        cursor: &mut TreeCursor,
        source: &str,
        file_path: &str,
        parent_id: Option<String>,
        symbols: &mut Vec<Symbol>,
    ) {
        let node = cursor.node();
        
        // Check if this node represents a symbol
        if let Some(symbol_type) = self.language_config.node_to_symbol_type(node.kind()) {
            let symbol = self.extract_symbol(
                node,
                source,
                file_path,
                parent_id.clone(),
                symbol_type,
            );
            
            let symbol_id = symbol.id.clone();
            symbols.push(symbol);
            
            // Recurse with this symbol as parent
            if cursor.goto_first_child() {
                loop {
                    self.extract_symbols_recursive(
                        cursor,
                        source,
                        file_path,
                        Some(symbol_id.clone()),
                        symbols,
                    );
                    
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
                cursor.goto_parent();
            }
        } else {
            // Not a symbol, but recurse to find symbols in children
            if cursor.goto_first_child() {
                loop {
                    self.extract_symbols_recursive(
                        cursor,
                        source,
                        file_path,
                        parent_id.clone(),
                        symbols,
                    );
                    
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
                cursor.goto_parent();
            }
        }
    }
    
    fn extract_symbol(
        &self,
        node: Node,
        source: &str,
        file_path: &str,
        parent_id: Option<String>,
        symbol_type: SymbolType,
    ) -> Symbol {
        let name = self.extract_symbol_name(node, source);
        let byte_range = (node.start_byte(), node.end_byte());
        let line_range = (
            node.start_position().row + 1,
            node.end_position().row + 1,
        );
        
        let id = format!("{}:{}:{}", file_path, name, node.start_byte());
        
        Symbol {
            id,
            name,
            symbol_type,
            file_path: file_path.to_string(),
            byte_range,
            line_range,
            parent_id,
            children: Vec::new(),
            dependencies: self.extract_dependencies(node, source),
            docstring: self.extract_docstring(node, source),
            metadata: self.extract_metadata(node, source),
        }
    }
}
```

### 2. Symbol Index (SQLite)

#### Schema

```sql
-- .zblade/index/symbols.db

-- Symbols table
CREATE TABLE symbols (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    symbol_type TEXT NOT NULL,
    file_path TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    parent_id TEXT,
    docstring TEXT,
    is_exported BOOLEAN DEFAULT 0,
    is_async BOOLEAN DEFAULT 0,
    visibility TEXT,
    complexity INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (parent_id) REFERENCES symbols(id) ON DELETE CASCADE
);

CREATE INDEX idx_symbols_name ON symbols(name);
CREATE INDEX idx_symbols_file ON symbols(file_path);
CREATE INDEX idx_symbols_type ON symbols(symbol_type);
CREATE INDEX idx_symbols_parent ON symbols(parent_id);

-- Full-text search on symbols
CREATE VIRTUAL TABLE symbols_fts USING fts5(
    name,
    docstring,
    content=symbols,
    content_rowid=rowid
);

-- Dependencies table
CREATE TABLE dependencies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol_id TEXT NOT NULL,
    dep_type TEXT NOT NULL,
    target TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    FOREIGN KEY (symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
);

CREATE INDEX idx_deps_symbol ON dependencies(symbol_id);
CREATE INDEX idx_deps_target ON dependencies(target);
CREATE INDEX idx_deps_type ON dependencies(dep_type);

-- File metadata (track parsing status)
CREATE TABLE file_metadata (
    file_path TEXT PRIMARY KEY,
    language TEXT NOT NULL,
    last_parsed TIMESTAMP NOT NULL,
    parse_duration_ms INTEGER,
    symbol_count INTEGER DEFAULT 0,
    has_errors BOOLEAN DEFAULT 0,
    file_hash TEXT NOT NULL
);

CREATE INDEX idx_file_language ON file_metadata(language);
CREATE INDEX idx_file_parsed ON file_metadata(last_parsed DESC);

-- Symbol references (for find-all-references)
CREATE TABLE symbol_references (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol_id TEXT NOT NULL,
    reference_file TEXT NOT NULL,
    reference_line INTEGER NOT NULL,
    reference_byte INTEGER NOT NULL,
    context TEXT,
    FOREIGN KEY (symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
);

CREATE INDEX idx_refs_symbol ON symbol_references(symbol_id);
CREATE INDEX idx_refs_file ON symbol_references(reference_file);
```

#### Index Manager

```rust
// src-tauri/src/tree_sitter/index.rs

pub struct SymbolIndex {
    db: Connection,
    workspace_root: PathBuf,
}

impl SymbolIndex {
    pub fn new(workspace_root: PathBuf) -> Result<Self, Error> {
        let db_path = workspace_root.join(".zblade/index/symbols.db");
        
        // Create directory if needed
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let db = Connection::open(db_path)?;
        
        // Initialize schema
        Self::init_schema(&db)?;
        
        Ok(Self {
            db,
            workspace_root,
        })
    }
    
    pub fn index_file(&mut self, file_path: &Path) -> Result<(), Error> {
        let source = fs::read_to_string(file_path)?;
        let language = detect_language(file_path)?;
        
        let mut extractor = SymbolExtractor::new(language)?;
        let symbols = extractor.extract_symbols(
            &source,
            file_path.strip_prefix(&self.workspace_root)?.to_str().unwrap()
        )?;
        
        // Begin transaction
        let tx = self.db.transaction()?;
        
        // Delete old symbols for this file
        tx.execute(
            "DELETE FROM symbols WHERE file_path = ?1",
            params![file_path.to_str().unwrap()],
        )?;
        
        // Insert new symbols
        for symbol in symbols {
            self.insert_symbol(&tx, &symbol)?;
        }
        
        // Update file metadata
        tx.execute(
            "INSERT OR REPLACE INTO file_metadata 
             (file_path, language, last_parsed, symbol_count, file_hash)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                file_path.to_str().unwrap(),
                format!("{:?}", language),
                chrono::Utc::now(),
                symbols.len(),
                hash_file(&source),
            ],
        )?;
        
        tx.commit()?;
        
        Ok(())
    }
    
    pub fn search_symbols(&self, query: &str) -> Result<Vec<Symbol>, Error> {
        let mut stmt = self.db.prepare(
            "SELECT * FROM symbols 
             WHERE name LIKE ?1 OR id IN (
                 SELECT rowid FROM symbols_fts WHERE symbols_fts MATCH ?1
             )
             ORDER BY name
             LIMIT 100"
        )?;
        
        let symbols = stmt.query_map(params![format!("%{}%", query)], |row| {
            self.row_to_symbol(row)
        })?
        .collect::<Result<Vec<_>, _>>()?;
        
        Ok(symbols)
    }
    
    pub fn get_symbols_in_file(&self, file_path: &str) -> Result<Vec<Symbol>, Error> {
        let mut stmt = self.db.prepare(
            "SELECT * FROM symbols WHERE file_path = ?1 ORDER BY start_line"
        )?;
        
        let symbols = stmt.query_map(params![file_path], |row| {
            self.row_to_symbol(row)
        })?
        .collect::<Result<Vec<_>, _>>()?;
        
        Ok(symbols)
    }
    
    pub fn get_symbol_at_position(
        &self,
        file_path: &str,
        line: usize,
    ) -> Result<Option<Symbol>, Error> {
        let mut stmt = self.db.prepare(
            "SELECT * FROM symbols 
             WHERE file_path = ?1 
               AND start_line <= ?2 
               AND end_line >= ?2
             ORDER BY (end_line - start_line) ASC
             LIMIT 1"
        )?;
        
        let symbol = stmt.query_row(params![file_path, line], |row| {
            self.row_to_symbol(row)
        }).optional()?;
        
        Ok(symbol)
    }
}
```

### 3. Semantic Code References (RFC-002 Enhancement)

#### New Reference Format

```typescript
// Frontend: src/types/semantic-reference.ts

export interface SemanticCodeReference {
  // Stable identifier (survives refactoring)
  symbol_id: string;
  
  // Human-readable info
  symbol_name: string;
  symbol_type: "function" | "class" | "method" | "variable";
  
  // File location (may change)
  file_path: string;
  line_range: [number, number];
  
  // Context for AI
  context: {
    // Parent context (class for method)
    parent?: {
      name: string;
      type: string;
    };
    
    // Dependencies needed to understand this symbol
    dependencies: Array<{
      type: "import" | "call" | "type";
      target: string;
    }>;
    
    // Docstring/comments
    documentation?: string;
    
    // Complexity metrics
    complexity?: number;
  };
  
  // Fallback if symbol no longer exists
  fallback_content?: string;
}
```

#### Context Assembly with Tree-sitter

```typescript
// Frontend: src/lib/context-assembly.ts

export class SemanticContextAssembler {
  private parser: Parser;
  private symbolIndex: SymbolIndex;
  
  async assembleContext(
    references: SemanticCodeReference[],
    maxTokens: number
  ): Promise<AssembledContext> {
    const symbols: Symbol[] = [];
    
    for (const ref of references) {
      // Try to find symbol by ID (stable)
      let symbol = await this.symbolIndex.getSymbol(ref.symbol_id);
      
      if (!symbol) {
        // Symbol moved/renamed, try to find by name and location
        symbol = await this.symbolIndex.getSymbolAt(
          ref.file_path,
          ref.line_range[0]
        );
      }
      
      if (symbol) {
        symbols.push(symbol);
      } else {
        // Use fallback content
        console.warn(`Symbol ${ref.symbol_id} not found, using fallback`);
      }
    }
    
    // Extract minimal context for each symbol
    const contextParts = await Promise.all(
      symbols.map(s => this.extractMinimalContext(s))
    );
    
    // Compress to fit token budget
    return this.compressContext(contextParts, maxTokens);
  }
  
  private async extractMinimalContext(symbol: Symbol): Promise<ContextPart> {
    // Read file
    const content = await readFile(symbol.file_path);
    
    // Parse to get AST
    const tree = this.parser.parse(content);
    const node = this.findNodeByRange(tree, symbol.byte_range);
    
    if (!node) {
      return { symbol, content: symbol.fallback_content || "" };
    }
    
    // Extract only what's needed
    const extracted = {
      symbol_name: symbol.name,
      symbol_type: symbol.symbol_type,
      
      // The actual code
      code: this.getNodeText(node, content),
      
      // Dependencies (imports needed)
      imports: this.extractImports(tree, symbol.dependencies),
      
      // Type definitions (if TypeScript/Rust)
      types: this.extractTypeDefinitions(tree, symbol),
      
      // Documentation
      docstring: symbol.docstring,
    };
    
    return {
      symbol,
      content: this.formatForAI(extracted),
      tokens: estimateTokens(extracted),
    };
  }
  
  private compressContext(
    parts: ContextPart[],
    maxTokens: number
  ): AssembledContext {
    // Sort by importance (user-selected > dependencies)
    parts.sort((a, b) => b.importance - a.importance);
    
    let totalTokens = 0;
    const included: ContextPart[] = [];
    
    for (const part of parts) {
      if (totalTokens + part.tokens <= maxTokens) {
        included.push(part);
        totalTokens += part.tokens;
      } else {
        // Try to include summary instead
        const summary = this.summarizeSymbol(part.symbol);
        const summaryTokens = estimateTokens(summary);
        
        if (totalTokens + summaryTokens <= maxTokens) {
          included.push({
            ...part,
            content: summary,
            tokens: summaryTokens,
            is_summary: true,
          });
          totalTokens += summaryTokens;
        }
      }
    }
    
    return {
      parts: included,
      total_tokens: totalTokens,
      compression_ratio: parts.length / included.length,
    };
  }
}
```

### 4. Semantic Diff Application (Blade Protocol Enhancement)

#### Semantic Patch Format

```typescript
// Shared type between zblade and zcoderd

export interface SemanticPatch {
  // Patch ID
  id: string;
  
  // Target symbol to modify
  target: {
    // Stable identifier
    symbol_id?: string;
    
    // Fallback: find by name and type
    symbol_name: string;
    symbol_type: "function" | "class" | "method";
    
    // Fallback: find by file and line
    file_path: string;
    approximate_line?: number;
  };
  
  // Operation type
  operation: "replace" | "insert_before" | "insert_after" | "wrap" | "delete";
  
  // New code to apply
  new_code: string;
  
  // Expected old code (for validation)
  expected_old_code?: string;
  
  // Metadata
  metadata: {
    // AI model that generated this
    model: string;
    
    // Confidence score
    confidence: number;
    
    // Description of change
    description: string;
  };
}
```

#### Semantic Patch Applier

```rust
// src-tauri/src/tree_sitter/patch_applier.rs

pub struct SemanticPatchApplier {
    parser: Parser,
    symbol_index: SymbolIndex,
}

impl SemanticPatchApplier {
    pub fn apply_patch(
        &mut self,
        patch: &SemanticPatch,
    ) -> Result<ApplyResult, Error> {
        // 1. Find target symbol
        let symbol = self.find_target_symbol(&patch.target)?;
        
        // 2. Read file
        let file_path = self.workspace_root.join(&symbol.file_path);
        let content = fs::read_to_string(&file_path)?;
        
        // 3. Parse file
        let tree = self.parser.parse(&content, None)
            .ok_or(Error::ParseFailed)?;
        
        // 4. Find target node
        let node = self.find_node_by_symbol(&tree, &symbol)?;
        
        // 5. Validate expected old code (if provided)
        if let Some(expected) = &patch.expected_old_code {
            let actual = self.get_node_text(node, &content);
            if !self.code_matches(expected, &actual) {
                return Err(Error::CodeMismatch {
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        
        // 6. Validate new code syntax
        self.validate_new_code(&patch.new_code)?;
        
        // 7. Apply operation
        let new_content = match patch.operation {
            Operation::Replace => {
                self.replace_node(&content, node, &patch.new_code)
            }
            Operation::InsertBefore => {
                self.insert_before_node(&content, node, &patch.new_code)
            }
            Operation::InsertAfter => {
                self.insert_after_node(&content, node, &patch.new_code)
            }
            Operation::Wrap => {
                self.wrap_node(&content, node, &patch.new_code)
            }
            Operation::Delete => {
                self.delete_node(&content, node)
            }
        }?;
        
        // 8. Validate result parses correctly
        self.validate_result(&new_content)?;
        
        // 9. Write back to file
        fs::write(&file_path, &new_content)?;
        
        // 10. Re-index file
        self.symbol_index.index_file(&file_path)?;
        
        Ok(ApplyResult {
            success: true,
            file_path: symbol.file_path.clone(),
            old_content: content,
            new_content,
        })
    }
    
    fn find_target_symbol(&self, target: &PatchTarget) -> Result<Symbol, Error> {
        // Try by symbol_id first (most stable)
        if let Some(symbol_id) = &target.symbol_id {
            if let Some(symbol) = self.symbol_index.get_symbol(symbol_id)? {
                return Ok(symbol);
            }
        }
        
        // Try by name and type
        let candidates = self.symbol_index.search_symbols(&target.symbol_name)?;
        let filtered: Vec<_> = candidates.into_iter()
            .filter(|s| s.symbol_type == target.symbol_type)
            .collect();
        
        if filtered.len() == 1 {
            return Ok(filtered[0].clone());
        }
        
        // Try by file and approximate line
        if let Some(line) = target.approximate_line {
            if let Some(symbol) = self.symbol_index.get_symbol_at_position(
                &target.file_path,
                line,
            )? {
                if symbol.name == target.symbol_name {
                    return Ok(symbol);
                }
            }
        }
        
        Err(Error::SymbolNotFound {
            name: target.symbol_name.clone(),
            file: target.file_path.clone(),
        })
    }
    
    fn validate_new_code(&mut self, code: &str) -> Result<(), Error> {
        let tree = self.parser.parse(code, None)
            .ok_or(Error::ParseFailed)?;
        
        if tree.root_node().has_error() {
            return Err(Error::InvalidSyntax {
                code: code.to_string(),
            });
        }
        
        Ok(())
    }
}
```

### 5. Frontend Integration (CodeMirror)

#### Tree-sitter Extension for CodeMirror

```typescript
// src/components/editor/extensions/treeSitter.ts

import Parser from "web-tree-sitter";

export interface TreeSitterConfig {
  language: string;
  parser: Parser;
}

export function createTreeSitterExtension(config: TreeSitterConfig) {
  return StateField.define<Tree | null>({
    create(state) {
      const tree = config.parser.parse(state.doc.toString());
      return tree;
    },
    
    update(tree, tr) {
      if (!tr.docChanged || !tree) {
        return tree;
      }
      
      // Incremental parsing
      for (const change of tr.changes) {
        tree.edit({
          startIndex: change.from,
          oldEndIndex: change.from + change.length,
          newEndIndex: change.from + change.insert.length,
          startPosition: posToPoint(state.doc.lineAt(change.from)),
          oldEndPosition: posToPoint(state.doc.lineAt(change.from + change.length)),
          newEndPosition: posToPoint(state.doc.lineAt(change.from + change.insert.length)),
        });
      }
      
      const newTree = config.parser.parse(tr.state.doc.toString(), tree);
      return newTree;
    },
  });
}

// Semantic folding based on tree-sitter
export function semanticFolding(tree: Tree) {
  const foldableRanges: Array<{from: number, to: number}> = [];
  
  const cursor = tree.walk();
  
  function visit() {
    const node = cursor.currentNode();
    
    // Fold functions, classes, blocks
    if (isFoldable(node.type)) {
      foldableRanges.push({
        from: node.startIndex,
        to: node.endIndex,
      });
    }
    
    if (cursor.gotoFirstChild()) {
      do {
        visit();
      } while (cursor.gotoNextSibling());
      cursor.gotoParent();
    }
  }
  
  visit();
  
  return foldableRanges;
}

function isFoldable(nodeType: string): boolean {
  return [
    "function_declaration",
    "class_declaration",
    "method_definition",
    "block_statement",
    "object_expression",
    "array_expression",
  ].includes(nodeType);
}
```

---

## Implementation Phases

### Phase 1: Foundation (Weeks 1-2)

**Week 1: Rust Backend Setup**
- [ ] Add tree-sitter dependencies to Cargo.toml
- [ ] Add language grammars (TS, JS, Rust, Python, Go)
- [ ] Create `src-tauri/src/tree_sitter/` module
- [ ] Implement basic parser wrapper
- [ ] Write unit tests for parsing

**Week 2: Frontend Setup & Symbol Extraction**
- [ ] Add web-tree-sitter to package.json
- [ ] Load WASM parsers in frontend
- [ ] Implement `SymbolExtractor` in Rust
- [ ] Create symbol data structures
- [ ] Test symbol extraction on sample files

**Deliverables:**
- ✅ Tree-sitter integrated in both stacks
- ✅ Can parse 10 core languages
- ✅ Symbol extraction working
- ✅ Unit tests passing

---

### Phase 2: Symbol Index (Weeks 3-4)

**Week 3: SQLite Schema & Index Manager**
- [ ] Create `.zblade/index/symbols.db` schema
- [ ] Implement `SymbolIndex` in Rust
- [ ] Add Tauri commands for indexing
- [ ] Implement incremental indexing (only changed files)
- [ ] Add file watcher integration

**Week 4: Search & Query API**
- [ ] Implement symbol search (by name, type)
- [ ] Implement get-symbol-at-position
- [ ] Add full-text search on symbols
- [ ] Create frontend hooks (`useSymbolSearch`)
- [ ] Build symbol search UI component

**Deliverables:**
- ✅ Symbol index persisted in SQLite
- ✅ Fast symbol search (<50ms)
- ✅ Incremental updates on file changes
- ✅ Search UI in editor

---

### Phase 3: RFC-002 Enhancement (Weeks 5-6)

**Week 5: Semantic Code References**
- [ ] Update conversation artifact format
- [ ] Implement semantic reference extraction
- [ ] Update context assembly to use symbols
- [ ] Migrate existing line-based references
- [ ] Test with real conversations

**Week 6: Context Compression**
- [ ] Implement minimal context extraction
- [ ] Add dependency resolution
- [ ] Implement context compression algorithm
- [ ] Measure token reduction
- [ ] Update `.zblade/` storage format

**Deliverables:**
- ✅ Semantic references in conversations
- ✅ 40-60% token reduction in context
- ✅ Stable references across refactors
- ✅ Backward compatibility with old format

---

### Phase 4: Blade Protocol Enhancement (Weeks 7-8)

**Week 7: Semantic Patch Format**
- [ ] Define `SemanticPatch` type
- [ ] Update Blade Protocol spec
- [ ] Implement patch serialization
- [ ] Update zcoderd to generate semantic patches
- [ ] Test patch generation

**Week 8: Patch Application**
- [ ] Implement `SemanticPatchApplier`
- [ ] Add syntax validation
- [ ] Add conflict detection
- [ ] Implement rollback on failure
- [ ] Test with complex refactorings

**Deliverables:**
- ✅ Semantic patches in Blade Protocol
- ✅ 80% improvement in patch reliability
- ✅ Syntax validation before apply
- ✅ Graceful failure handling

---

### Phase 5: Editor Features (Weeks 9-10)

**Week 9: Semantic Folding & Navigation**
- [ ] Implement semantic code folding
- [ ] Add breadcrumb navigation (show current symbol)
- [ ] Implement jump-to-symbol
- [ ] Add outline view (symbol tree)
- [ ] Test with large files

**Week 10: Structural Search**
- [ ] Implement query language for structural search
- [ ] Add search UI
- [ ] Implement find-all-references
- [ ] Add code metrics display
- [ ] Polish UX

**Deliverables:**
- ✅ Semantic folding in editor
- ✅ Symbol navigation working
- ✅ Structural search functional
- ✅ Enhanced developer experience

---

## API Specifications

### Tauri Commands

```rust
// src-tauri/src/lib.rs

#[tauri::command]
async fn index_workspace(
    workspace_path: String,
    state: State<'_, AppState>,
) -> Result<IndexResult, String> {
    // Index all files in workspace
}

#[tauri::command]
async fn index_file(
    file_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Index single file
}

#[tauri::command]
async fn search_symbols(
    query: String,
    filters: SymbolFilters,
    state: State<'_, AppState>,
) -> Result<Vec<Symbol>, String> {
    // Search symbols by name/type
}

#[tauri::command]
async fn get_symbol_at_position(
    file_path: String,
    line: usize,
    state: State<'_, AppState>,
) -> Result<Option<Symbol>, String> {
    // Get symbol at cursor position
}

#[tauri::command]
async fn get_symbols_in_file(
    file_path: String,
    state: State<'_, AppState>,
) -> Result<Vec<Symbol>, String> {
    // Get all symbols in file (for outline)
}

#[tauri::command]
async fn apply_semantic_patch(
    patch: SemanticPatch,
    state: State<'_, AppState>,
) -> Result<ApplyResult, String> {
    // Apply semantic patch to code
}

#[tauri::command]
async fn validate_code(
    code: String,
    language: String,
    state: State<'_, AppState>,
) -> Result<ValidationResult, String> {
    // Validate code syntax
}

#[tauri::command]
async fn extract_symbol_context(
    symbol_id: String,
    include_dependencies: bool,
    state: State<'_, AppState>,
) -> Result<SymbolContext, String> {
    // Extract minimal context for symbol
}
```

### Frontend Hooks

```typescript
// src/hooks/useTreeSitter.ts

export function useSymbolIndex() {
  const indexWorkspace = async (path: string) => {
    return invoke<IndexResult>("index_workspace", { workspacePath: path });
  };
  
  const searchSymbols = async (query: string, filters?: SymbolFilters) => {
    return invoke<Symbol[]>("search_symbols", { query, filters });
  };
  
  const getSymbolAtPosition = async (filePath: string, line: number) => {
    return invoke<Symbol | null>("get_symbol_at_position", { filePath, line });
  };
  
  return {
    indexWorkspace,
    searchSymbols,
    getSymbolAtPosition,
  };
}

export function useSemanticContext() {
  const assembleContext = async (
    references: SemanticCodeReference[],
    maxTokens: number
  ) => {
    // Assemble context from semantic references
  };
  
  const extractSymbolContext = async (symbolId: string) => {
    return invoke<SymbolContext>("extract_symbol_context", { 
      symbolId,
      includeDependencies: true 
    });
  };
  
  return {
    assembleContext,
    extractSymbolContext,
  };
}
```

---

## Data Models

### Conversation Artifact (Updated)

```json
{
  "version": "2.0",
  "conversation_id": "conv_abc123",
  "messages": [
    {
      "id": "msg_001",
      "role": "user",
      "content": "How do I implement JWT auth?",
      "code_references": [
        {
          "type": "semantic",
          "symbol_id": "src/auth.ts:authenticate:1234",
          "symbol_name": "authenticate",
          "symbol_type": "function",
          "file_path": "src/auth.ts",
          "line_range": [10, 25],
          "context": {
            "parent": null,
            "dependencies": [
              { "type": "import", "target": "jsonwebtoken" },
              { "type": "type", "target": "User" }
            ],
            "documentation": "Authenticates user with JWT token",
            "complexity": 5
          }
        }
      ]
    }
  ]
}
```

---

## Integration Points

### 1. RFC-002 Local Storage

**Before:**
```typescript
// Load conversation, get line-based references
const ref = { file: "src/auth.ts", lines: [10, 25] };
const code = readLines(ref.file, ref.lines[0], ref.lines[1]);
```

**After:**
```typescript
// Load conversation, get semantic references
const ref = { symbol_id: "src/auth.ts:authenticate:1234" };
const symbol = await symbolIndex.getSymbol(ref.symbol_id);
const context = await extractSymbolContext(symbol);
// Context includes only relevant code + dependencies
```

### 2. Blade Protocol

**Before:**
```json
{
  "type": "propose_change",
  "change_type": "patch",
  "path": "src/auth.ts",
  "old_content": "...",
  "new_content": "..."
}
```

**After:**
```json
{
  "type": "propose_change",
  "change_type": "semantic_patch",
  "patch": {
    "target": {
      "symbol_id": "src/auth.ts:authenticate:1234",
      "symbol_name": "authenticate",
      "symbol_type": "function"
    },
    "operation": "replace",
    "new_code": "..."
  }
}
```

### 3. Editor Features

**New CodeMirror Extensions:**
- `treeSitterField` - Maintains AST state
- `semanticFolding` - Fold by function/class
- `symbolNavigation` - Jump to symbol
- `breadcrumbs` - Show current symbol path

---

## Performance Considerations

### Parsing Performance

**Benchmarks (estimated):**
- Small file (100 lines): <5ms
- Medium file (1000 lines): <20ms
- Large file (10000 lines): <100ms
- Incremental re-parse: <10ms (only changed sections)

**Optimization strategies:**
- Use incremental parsing (only re-parse changed sections)
- Parse in background thread (Rust: tokio, Frontend: Web Worker)
- Cache parse trees in memory
- Lazy symbol extraction (only when needed)

### Index Performance

**Target metrics:**
- Index 1000 files: <10 seconds
- Symbol search: <50ms
- Get symbol at position: <10ms
- Incremental update: <100ms per file

**Optimization strategies:**
- Use SQLite indexes on frequently queried columns
- Batch inserts in transactions
- Use FTS5 for full-text search
- Cache frequently accessed symbols

### Memory Usage

**Estimates:**
- Parse tree: ~100KB per 1000 lines
- Symbol index: ~10KB per 100 symbols
- Total for 10K file project: ~50MB

**Optimization strategies:**
- Don't keep all parse trees in memory
- Use weak references for cached trees
- Implement LRU cache for symbols
- Compress stored ASTs

---

## Migration Strategy

### Backward Compatibility

**Support both formats during transition:**

```typescript
interface CodeReference {
  // New format
  type: "semantic" | "line-based";
  
  // Semantic reference
  symbol_id?: string;
  symbol_name?: string;
  symbol_type?: string;
  context?: SymbolContext;
  
  // Line-based reference (legacy)
  file?: string;
  lines?: [number, number];
  
  // Fallback content
  fallback_content?: string;
}
```

### Migration Process

**Phase 1: Dual-mode (Weeks 1-4)**
- Support both formats
- New conversations use semantic references
- Old conversations use line-based references
- No breaking changes

**Phase 2: Migration Tool (Week 5)**
- Build migration tool to convert old references
- Run on user's existing conversations
- Preserve fallback content for safety

**Phase 3: Deprecation (Week 8+)**
- Mark line-based format as deprecated
- Encourage users to migrate
- Keep support for 6 months

---

## Testing Strategy

### Unit Tests

```rust
// src-tauri/src/tree_sitter/tests.rs

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_extract_function_symbols() {
        let source = r#"
            function authenticate(token: string) {
                return jwt.verify(token);
            }
        "#;
        
        let mut extractor = SymbolExtractor::new(Language::TypeScript).unwrap();
        let symbols = extractor.extract_symbols(source, "test.ts").unwrap();
        
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "authenticate");
        assert_eq!(symbols[0].symbol_type, SymbolType::Function);
    }
    
    #[test]
    fn test_semantic_patch_application() {
        // Test patch application
    }
    
    #[test]
    fn test_symbol_index_search() {
        // Test symbol search
    }
}
```

### Integration Tests

```typescript
// src/tests/tree-sitter.test.ts

describe("Tree-sitter Integration", () => {
  test("should index workspace", async () => {
    const result = await invoke("index_workspace", {
      workspacePath: "/test/workspace"
    });
    
    expect(result.files_indexed).toBeGreaterThan(0);
  });
  
  test("should search symbols", async () => {
    const symbols = await invoke("search_symbols", {
      query: "auth"
    });
    
    expect(symbols).toContainEqual(
      expect.objectContaining({ name: "authenticate" })
    );
  });
  
  test("should apply semantic patch", async () => {
    const patch = {
      target: { symbol_name: "authenticate", symbol_type: "function" },
      operation: "replace",
      new_code: "..."
    };
    
    const result = await invoke("apply_semantic_patch", { patch });
    expect(result.success).toBe(true);
  });
});
```

### Performance Tests

```rust
#[bench]
fn bench_parse_large_file(b: &mut Bencher) {
    let source = generate_large_file(10000); // 10K lines
    let mut parser = Parser::new();
    parser.set_language(Language::TypeScript).unwrap();
    
    b.iter(|| {
        parser.parse(&source, None);
    });
}

#[bench]
fn bench_incremental_parse(b: &mut Bencher) {
    // Test incremental parsing performance
}

#[bench]
fn bench_symbol_search(b: &mut Bencher) {
    // Test search performance
}
```

---

## Security & Privacy

### Local-First Parsing

**Privacy benefits:**
- ✅ All parsing happens locally (frontend or Rust backend)
- ✅ No code sent to external servers for parsing
- ✅ Symbol index stored in `.zblade/` (user controls)
- ✅ Compatible with RFC-002 local storage mode

### Validation

**Security measures:**
- Validate all AI-generated code before applying
- Sandbox parsing (tree-sitter is memory-safe)
- Limit parse tree size (prevent DoS)
- Validate file paths (prevent directory traversal)

---

## Success Metrics

### Performance Metrics

- **Context token reduction**: 40-60% (target: 50%)
- **Patch application success rate**: 80%+ (vs 60% baseline)
- **Parse time**: <100ms for 10K line file
- **Index time**: <10s for 1000 files
- **Search latency**: <50ms

### User Experience Metrics

- **Semantic folding adoption**: 70% of users
- **Symbol search usage**: 50+ searches/day/user
- **Context assembly time**: <200ms
- **User satisfaction**: 4.5/5 stars

### Code Quality Metrics

- **Syntax errors caught**: 95% before application
- **Refactoring stability**: 90% of references survive
- **False positives**: <5% in symbol search

---

## Future Enhancements

### Short-term (3-6 months)

- **Semantic code completion**: Use AST for smarter autocomplete
- **Refactoring tools**: Extract function, inline variable, etc.
- **Code metrics dashboard**: Complexity, coupling, coverage
- **Cross-file analysis**: Find all usages across workspace

### Medium-term (6-12 months)

- **Custom query language**: Structural search DSL
- **AI-powered refactoring**: Suggest refactorings based on patterns
- **Code smell detection**: Identify anti-patterns
- **Dependency graph visualization**: Show symbol relationships

### Long-term (12+ months)

- **Multi-repo support**: Index across multiple repositories
- **Team knowledge base**: Share semantic annotations
- **Code evolution tracking**: Track how symbols change over time
- **Semantic diff visualization**: Show structural changes, not just text

---

## Open Questions

1. **Should we use tree-sitter for syntax highlighting?**
   - Pro: Consistent with parsing
   - Con: CodeMirror's Lezer is already good
   - Decision: Keep Lezer for now, evaluate later

2. **How to handle very large files (>100K lines)?**
   - Option A: Lazy parsing (parse on-demand)
   - Option B: Streaming parsing (parse in chunks)
   - Option C: Warn user and skip
   - Decision: TBD based on testing

3. **Should symbol index be per-workspace or global?**
   - Pro (per-workspace): Simpler, more isolated
   - Pro (global): Cross-project search
   - Decision: Per-workspace for now

4. **How to handle generated files (node_modules, build output)?**
   - Option A: Respect .gitignore
   - Option B: Separate ignore file (.zbladeignore)
   - Decision: Use .gitignore + .zbladeignore

---

## Dependencies

### Rust Dependencies

```toml
# Add to src-tauri/Cargo.toml

[dependencies]
tree-sitter = "0.20"
tree-sitter-typescript = "0.20"
tree-sitter-javascript = "0.20"
tree-sitter-rust = "0.20"
tree-sitter-python = "0.20"
tree-sitter-go = "0.20"
tree-sitter-html = "0.20"
tree-sitter-css = "0.20"
tree-sitter-json = "0.20"
tree-sitter-yaml = "0.20"
tree-sitter-markdown = "0.20"
```

### Frontend Dependencies

```json
{
  "dependencies": {
    "web-tree-sitter": "^0.20.8"
  },
  "devDependencies": {
    "@types/web-tree-sitter": "^0.20.1"
  }
}
```

---

## References

- [Tree-sitter Documentation](https://tree-sitter.github.io/tree-sitter/)
- [RFC-002: Client-side Unlimited Context](./RFC-002-client-side-unlimited-context.md)
- [Blade Protocol Specification](./BLADE_CHANGE_PROTOCOL.md)
- [ZaguanBlade Architecture](./ARCHITECTURE.md)
- [Tree-sitter Rust Bindings](https://github.com/tree-sitter/tree-sitter/tree/master/lib/binding_rust)
- [web-tree-sitter](https://github.com/tree-sitter/tree-sitter/tree/master/lib/binding_web)

---

## Appendix A: Language-Specific Configurations

### TypeScript/JavaScript

```rust
pub struct TypeScriptConfig;

impl LanguageConfig for TypeScriptConfig {
    fn node_to_symbol_type(&self, kind: &str) -> Option<SymbolType> {
        match kind {
            "function_declaration" => Some(SymbolType::Function),
            "method_definition" => Some(SymbolType::Method),
            "class_declaration" => Some(SymbolType::Class),
            "interface_declaration" => Some(SymbolType::Interface),
            "variable_declaration" => Some(SymbolType::Variable),
            "type_alias_declaration" => Some(SymbolType::Type),
            _ => None,
        }
    }
    
    fn extract_symbol_name(&self, node: Node, source: &str) -> String {
        // Find identifier node
        let identifier = node.child_by_field_name("name")
            .or_else(|| node.child_by_field_name("identifier"));
        
        if let Some(id) = identifier {
            return self.get_node_text(id, source);
        }
        
        "anonymous".to_string()
    }
}
```

### Rust

```rust
pub struct RustConfig;

impl LanguageConfig for RustConfig {
    fn node_to_symbol_type(&self, kind: &str) -> Option<SymbolType> {
        match kind {
            "function_item" => Some(SymbolType::Function),
            "impl_item" => Some(SymbolType::Method),
            "struct_item" => Some(SymbolType::Struct),
            "enum_item" => Some(SymbolType::Enum),
            "trait_item" => Some(SymbolType::Interface),
            "type_item" => Some(SymbolType::Type),
            "const_item" => Some(SymbolType::Constant),
            "mod_item" => Some(SymbolType::Module),
            _ => None,
        }
    }
}
```

---

## Appendix B: Example Workflows

### Workflow 1: User asks about authentication

**Before (line-based):**
1. User: "How did we implement authentication?"
2. System searches text for "auth"
3. Returns files with "auth" in them
4. Loads entire file sections (2000 tokens)
5. Sends to AI

**After (semantic):**
1. User: "How did we implement authentication?"
2. System searches symbol index for "auth"
3. Finds `authenticate` function, `AuthService` class
4. Extracts minimal context (function + imports + types)
5. Sends 800 tokens to AI (60% reduction)

### Workflow 2: AI suggests code change

**Before (text-based):**
1. AI generates patch with line numbers
2. User's code changed since conversation started
3. Patch fails to apply
4. User manually fixes

**After (semantic):**
1. AI generates semantic patch targeting `authenticate` function
2. System finds function even if it moved
3. Validates new code syntax
4. Applies at AST level
5. Success!

### Workflow 3: Refactoring

**Before:**
1. User renames function `auth` → `authenticate`
2. All conversation references break
3. Context assembly fails

**After:**
1. User renames function
2. Symbol ID remains stable
3. References automatically updated
4. Context assembly works

---

## Appendix C: Performance Benchmarks

### Parse Performance (Estimated)

| File Size | Lines | Parse Time | Incremental |
|-----------|-------|------------|-------------|
| Small     | 100   | 3ms        | 1ms         |
| Medium    | 1,000 | 18ms       | 5ms         |
| Large     | 10,000| 95ms       | 15ms        |
| Huge      | 100,000| 850ms     | 80ms        |

### Index Performance (Estimated)

| Operation              | Time   | Notes                    |
|------------------------|--------|--------------------------|
| Index 100 files        | 1.5s   | ~15ms per file           |
| Index 1,000 files      | 12s    | Parallel processing      |
| Search symbols         | 35ms   | With SQLite FTS5         |
| Get symbol at position | 8ms    | Single query             |
| Incremental update     | 80ms   | Re-index changed file    |

### Memory Usage (Estimated)

| Component          | Size per Unit | 10K File Project |
|--------------------|---------------|------------------|
| Parse tree cache   | 100KB/1K lines| 100MB            |
| Symbol index (DB)  | 10KB/100 syms | 50MB             |
| In-memory cache    | 1KB/symbol    | 5MB              |
| **Total**          |               | **~155MB**       |

---

## Conclusion

Tree-sitter integration will transform ZaguanBlade from a text-based editor into a **semantic code understanding platform**. The benefits span across all major features:

- **RFC-002**: Stable, semantic code references
- **Blade Protocol**: Reliable, AST-aware patches
- **Editor**: Intelligent folding, navigation, search
- **AI**: Better context, validation, compression

**Recommendation**: Proceed with implementation following the 10-week phased approach outlined above.

**Next Steps:**
1. Review and approve this RFC
2. Set up development environment
3. Begin Phase 1 (Foundation)
4. Weekly progress reviews

---

**Document Version**: 1.0  
**Last Updated**: 2026-01-18  
**Status**: Draft - Awaiting Approval
