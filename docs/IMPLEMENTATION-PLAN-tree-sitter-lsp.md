# Implementation Plan: LSP + Tree-sitter Integration

## Status

**Active** - Ready for Implementation

## Timeline

**10 Weeks Total** - Phased rollout with validation gates

## Created

2026-01-18

---

## Overview

This document provides the **actionable implementation plan** for integrating LSP and tree-sitter into ZaguanBlade.

**Key Principles:**
1. **Rust-first architecture**: Everything that can be done in Rust should be done in Rust for maximum performance and minimal resource consumption
2. **Backend-heavy processing**: Heavy operations (parsing, indexing, symbol extraction) happen in Rust backend, not JavaScript frontend
3. **Incremental delivery**: Build incrementally, validate at each phase, ship working features continuously
4. **Performance targets**: 3-10x faster than traditional frontend-heavy approaches (Monaco/VS Code)

---

## References & Related Documentation

### Core RFCs (Required Reading)

**[RFC-002: Client-Side Unlimited Context](./RFC-002-client-side-unlimited-context.md)**
- Defines the hybrid storage architecture for conversations
- Introduces line-based references (which we're upgrading to symbol-based)
- Specifies local SQLite index structure
- **Critical for:** Understanding why semantic references are needed

**[RFC-003: Tree-sitter Integration](./RFC-003-tree-sitter-integration.md)**
- Complete technical specification for tree-sitter integration
- Symbol extraction data models and algorithms
- Tree-sitter query patterns for each language
- Performance benchmarks and optimization strategies
- **Critical for:** Week 1-2 implementation (tree-sitter backend)

**[RFC-004: LSP & Tree-sitter Integration Strategy](./RFC-004-lsp-tree-sitter-integration-strategy.md)**
- Defines clear responsibility boundaries between LSP and tree-sitter
- Unified symbol index architecture
- Conflict resolution strategies
- Integration patterns and best practices
- **Critical for:** Week 3-5 implementation (integration layer)

**[RFC-005: BCP Extensions for LSP & Tree-sitter](./RFC-005-bcp-extensions-for-lsp-tree-sitter.md)**
- Blade Change Protocol extensions for Language domain
- Performance optimization strategies (3-10x faster)
- Backend-heavy architecture rationale
- Complete API specifications for intents and events
- **Critical for:** Week 3 implementation (BCP integration)

### Architecture Documentation

**[ARCHITECTURE.md](./ARCHITECTURE.md)**
- Overall ZaguanBlade architecture principles
- Rust backend best practices
- Tauri integration patterns
- State management guidelines
- **Reference for:** Code structure and patterns

**[BLADE_CHANGE_PROTOCOL.md](./BLADE_CHANGE_PROTOCOL.md)**
- Current BCP v1.2 specification
- Intent/Event patterns
- Causality tracking and versioning
- Error handling model
- **Reference for:** Extending BCP with Language domain

**[CODEMIRROR_VS_MONACO_INTEGRATION.md](./CODEMIRROR_VS_MONACO_INTEGRATION.md)**
- Why CodeMirror instead of Monaco
- Custom integration challenges and solutions
- Dual-parser approach (Lezer + tree-sitter)
- State coordination strategies
- **Critical for:** Week 9 implementation (frontend integration)

### LSP-Specific Documentation

**[AI_LSP_INTEGRATION.md](./AI_LSP_INTEGRATION.md)**
- AI-driven diagnostic fixing
- LSP diagnostic analyzer patterns
- Auto-fix confidence scoring
- Batch fix strategies
- **Critical for:** Week 7-8 implementation (AI self-correction)

**[ROADMAP.md](./ROADMAP.md)**
- LSP integration strategy overview
- Language server lifecycle management
- Multi-language support plans
- **Reference for:** Long-term vision alignment

### Implementation Guides

**[zblade-implementation-guide.md](./zblade-implementation-guide.md)**
- General implementation patterns
- Code organization principles
- Testing strategies
- **Reference for:** Development best practices

**[EVENTS.md](./EVENTS.md)**
- Current event system documentation
- Event naming conventions
- Payload structures
- **Reference for:** Adding Language events

### Related Features

**[COMMAND_EXECUTION_FEATURE.md](./COMMAND_EXECUTION_FEATURE.md)**
- Terminal integration patterns
- Process lifecycle management
- **Reference for:** Similar async operation patterns

**[VIRTUAL_BUFFER_IMPLEMENTATION.md](./VIRTUAL_BUFFER_IMPLEMENTATION.md)**
- Virtual buffer architecture
- Editor state management
- **Reference for:** Editor integration patterns

**[MULTI_PATCH_IMPLEMENTATION_PLAN.md](./MULTI_PATCH_IMPLEMENTATION_PLAN.md)**
- Patch application strategies
- Multi-file change coordination
- **Reference for:** Week 7 semantic patch implementation

### Testing & Quality

**[IMPLEMENTATION_STATUS.md](./IMPLEMENTATION_STATUS.md)**
- Current feature status
- Known issues and limitations
- **Reference for:** Integration points

**[PROTOCOL_AUDIT_REPORT.md](./PROTOCOL_AUDIT_REPORT.md)**
- Protocol compliance patterns
- Audit checklist
- **Reference for:** Ensuring BCP compliance

### Research & Inspiration

**[UnlimitedContextResearch-2512.24601v1.pdf](./UnlimitedContextResearch-2512.24601v1.pdf)**
- Academic research on unlimited context
- Theoretical foundations
- **Background reading:** Context optimization strategies

**[INSPIRATION_ANALYSIS.md](./INSPIRATION_ANALYSIS.md)**
- Competitive analysis
- Feature comparisons
- **Reference for:** Understanding competitive landscape

---

## Quick Reference Guide

### By Implementation Phase

**Phase 1 (Weeks 1-3): Foundation**
- Primary: RFC-003 (tree-sitter), RFC-005 (BCP)
- Secondary: ARCHITECTURE.md, BLADE_CHANGE_PROTOCOL.md

**Phase 2 (Weeks 4-5): Symbol Index**
- Primary: RFC-003 (symbol index), RFC-004 (unified index)
- Secondary: RFC-002 (SQLite patterns)

**Phase 3 (Week 6): RFC-002 Integration**
- Primary: RFC-002 (context system), RFC-003 (semantic references)
- Secondary: RFC-004 (context assembly)

**Phase 4 (Weeks 7-8): AI Self-Correction**
- Primary: AI_LSP_INTEGRATION.md, RFC-003 (semantic patches)
- Secondary: RFC-004 (validation), MULTI_PATCH_IMPLEMENTATION_PLAN.md

**Phase 5 (Weeks 9-10): Frontend**
- Primary: CODEMIRROR_VS_MONACO_INTEGRATION.md
- Secondary: VIRTUAL_BUFFER_IMPLEMENTATION.md, EVENTS.md

### By Topic

**Tree-sitter:**
- RFC-003 (complete spec)
- CODEMIRROR_VS_MONACO_INTEGRATION.md (frontend integration)

**LSP:**
- RFC-004 (integration strategy)
- RFC-005 (BCP extensions)
- AI_LSP_INTEGRATION.md (AI features)
- ROADMAP.md (LSP strategy)

**BCP/Protocol:**
- BLADE_CHANGE_PROTOCOL.md (current spec)
- RFC-005 (Language domain extensions)
- PROTOCOL_AUDIT_REPORT.md (compliance)

**Performance:**
- RFC-005 (optimization strategies)
- RFC-003 (benchmarks)
- CODEMIRROR_VS_MONACO_INTEGRATION.md (frontend performance)

**AI Integration:**
- AI_LSP_INTEGRATION.md (diagnostic fixing)
- RFC-002 (context system)
- RFC-003 (semantic patches)

---

---

## Phase 1: Foundation (Weeks 1-3)

### Week 1: Tree-sitter Backend Setup

**Goal:** Get tree-sitter parsing working in Rust backend

#### Tasks

**1.1 Add Dependencies**
```toml
# src-tauri/Cargo.toml

[dependencies]
tree-sitter = "0.20"
tree-sitter-rust = "0.20"
tree-sitter-typescript = "0.20"
tree-sitter-javascript = "0.20"
tree-sitter-python = "0.20"
```

**1.2 Create Tree-sitter Module**
```
src-tauri/src/
├─ tree_sitter/
│  ├─ mod.rs           # Public API
│  ├─ parser.rs        # Parser management
│  ├─ symbol.rs        # Symbol extraction
│  └─ query.rs         # Tree-sitter queries
```

**1.3 Implement Basic Parser**
```rust
// src-tauri/src/tree_sitter/parser.rs

pub struct TreeSitterParser {
    parsers: HashMap<Language, Parser>,
}

impl TreeSitterParser {
    pub fn new() -> Result<Self, Error> {
        let mut parsers = HashMap::new();
        
        // Initialize parsers for each language
        let mut ts_parser = Parser::new();
        ts_parser.set_language(tree_sitter_typescript::language())?;
        parsers.insert(Language::TypeScript, ts_parser);
        
        // ... other languages
        
        Ok(Self { parsers })
    }
    
    pub fn parse(&mut self, code: &str, language: Language) -> Result<Tree, Error> {
        let parser = self.parsers.get_mut(&language)
            .ok_or(Error::UnsupportedLanguage)?;
        
        parser.parse(code, None)
            .ok_or(Error::ParseFailed)
    }
    
    pub fn parse_incremental(
        &mut self,
        code: &str,
        old_tree: &Tree,
        edit: &InputEdit,
        language: Language,
    ) -> Result<Tree, Error> {
        let parser = self.parsers.get_mut(&language)
            .ok_or(Error::UnsupportedLanguage)?;
        
        parser.parse(code, Some(old_tree))
            .ok_or(Error::ParseFailed)
    }
}
```

**1.4 Implement Symbol Extraction**
```rust
// src-tauri/src/tree_sitter/symbol.rs

pub struct SymbolExtractor {
    queries: HashMap<Language, Query>,
}

impl SymbolExtractor {
    pub fn extract_symbols(&self, tree: &Tree, source: &str, language: Language) -> Vec<Symbol> {
        let query = self.queries.get(&language).unwrap();
        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(query, tree.root_node(), source.as_bytes());
        
        let mut symbols = Vec::new();
        
        for m in matches {
            let symbol = self.match_to_symbol(m, source);
            symbols.push(symbol);
        }
        
        symbols
    }
    
    fn match_to_symbol(&self, m: QueryMatch, source: &str) -> Symbol {
        // Extract symbol details from match
        Symbol {
            id: generate_symbol_id(&m),
            name: extract_name(&m, source),
            symbol_type: determine_type(&m),
            range: m.captures[0].node.range(),
            // ... other fields
        }
    }
}
```

**1.5 Write Tests**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_typescript() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "function hello() { return 'world'; }";
        let tree = parser.parse(code, Language::TypeScript).unwrap();
        
        assert!(!tree.root_node().has_error());
    }
    
    #[test]
    fn test_extract_function_symbol() {
        let code = "function authenticate(token: string) { }";
        let symbols = extract_symbols(code, Language::TypeScript);
        
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "authenticate");
        assert_eq!(symbols[0].symbol_type, SymbolType::Function);
    }
}
```

**Deliverables:**
- ✅ Tree-sitter parsing working for TypeScript/JavaScript
- ✅ Symbol extraction working
- ✅ Unit tests passing
- ✅ Performance benchmark (<20ms for 1000 lines)

---

### Week 2: LSP Manager Setup

**Goal:** Get LSP communication working with language servers

#### Tasks

**2.1 Add Dependencies**
```toml
# src-tauri/Cargo.toml

[dependencies]
lsp-types = "0.94"
lsp-server = "0.7"
serde_json = "1.0"
```

**2.2 Create LSP Module**
```
src-tauri/src/
├─ lsp/
│  ├─ mod.rs           # Public API
│  ├─ manager.rs       # LSP server lifecycle
│  ├─ client.rs        # JSON-RPC client
│  ├─ types.rs         # Type conversions
│  └─ handlers.rs      # Response handlers
```

**2.3 Implement LSP Manager**
```rust
// src-tauri/src/lsp/manager.rs

pub struct LspManager {
    servers: HashMap<String, LspServer>,
}

pub struct LspServer {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    capabilities: ServerCapabilities,
    pending_requests: HashMap<RequestId, oneshot::Sender<Value>>,
}

impl LspManager {
    pub async fn spawn_server(&mut self, language: &str) -> Result<(), Error> {
        let command = self.get_server_command(language)?;
        
        let mut process = Command::new(&command.program)
            .args(&command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        
        let stdin = process.stdin.take().unwrap();
        let stdout = BufReader::new(process.stdout.take().unwrap());
        
        // Initialize server
        let capabilities = self.initialize_server(&mut stdin, &mut stdout).await?;
        
        let server = LspServer {
            process,
            stdin,
            stdout,
            capabilities,
            pending_requests: HashMap::new(),
        };
        
        self.servers.insert(language.to_string(), server);
        
        Ok(())
    }
    
    pub async fn send_request(
        &mut self,
        language: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, Error> {
        let server = self.servers.get_mut(language)
            .ok_or(Error::ServerNotFound)?;
        
        let id = self.next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        
        // Send request
        self.write_message(&mut server.stdin, &request).await?;
        
        // Wait for response
        let (tx, rx) = oneshot::channel();
        server.pending_requests.insert(id, tx);
        
        rx.await.map_err(|_| Error::RequestCancelled)
    }
    
    async fn read_responses(&mut self, language: &str) {
        let server = self.servers.get_mut(language).unwrap();
        
        loop {
            match self.read_message(&mut server.stdout).await {
                Ok(msg) => {
                    if let Some(id) = msg.get("id") {
                        // Response to request
                        if let Some(tx) = server.pending_requests.remove(&id) {
                            let _ = tx.send(msg["result"].clone());
                        }
                    } else {
                        // Notification (e.g., diagnostics)
                        self.handle_notification(msg).await;
                    }
                }
                Err(e) => {
                    error!("LSP read error: {}", e);
                    break;
                }
            }
        }
    }
}
```

**2.4 Implement Basic LSP Operations**
```rust
impl LspManager {
    pub async fn get_diagnostics(&mut self, file_path: &str) -> Result<Vec<Diagnostic>, Error> {
        let language = self.detect_language(file_path)?;
        
        // LSP servers send diagnostics via notifications
        // We just return cached diagnostics
        Ok(self.diagnostic_cache.get(file_path).cloned().unwrap_or_default())
    }
    
    pub async fn get_completions(
        &mut self,
        file_path: &str,
        position: Position,
    ) -> Result<Vec<CompletionItem>, Error> {
        let language = self.detect_language(file_path)?;
        
        let params = json!({
            "textDocument": {
                "uri": format!("file://{}", file_path)
            },
            "position": {
                "line": position.line,
                "character": position.character
            }
        });
        
        let response = self.send_request(&language, "textDocument/completion", params).await?;
        
        Ok(serde_json::from_value(response)?)
    }
}
```

**2.5 Write Tests**
```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_spawn_typescript_server() {
        let mut manager = LspManager::new();
        manager.spawn_server("typescript").await.unwrap();
        
        assert!(manager.servers.contains_key("typescript"));
    }
    
    #[tokio::test]
    async fn test_get_completions() {
        let mut manager = setup_test_server().await;
        
        let completions = manager.get_completions(
            "test.ts",
            Position { line: 0, character: 7 }
        ).await.unwrap();
        
        assert!(!completions.is_empty());
    }
}
```

**Deliverables:**
- ✅ LSP manager spawning language servers
- ✅ JSON-RPC communication working
- ✅ Basic operations (diagnostics, completions)
- ✅ Integration tests passing

---

### Week 3: BCP Language Domain

**Goal:** Extend BCP with Language domain for unified API

#### Tasks

**3.1 Update BCP Protocol**
```rust
// src-tauri/src/protocol/intent.rs

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum BladeIntent {
    Chat(ChatIntent),
    Editor(EditorIntent),
    Workflow(WorkflowIntent),
    Terminal(TerminalIntent),
    System(SystemIntent),
    Language(LanguageIntent),  // NEW
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum LanguageIntent {
    // Symbol operations
    IndexFile { file_path: String },
    SearchSymbols { query: String },
    GetSymbolAt { file_path: String, position: Position },
    
    // LSP operations
    GetCompletions { file_path: String, position: Position },
    GetHover { file_path: String, position: Position },
    GetDiagnostics { file_path: String },
}
```

**3.2 Update BCP Events**
```rust
// src-tauri/src/protocol/event.rs

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum BladeEvent {
    Chat(ChatEvent),
    Editor(EditorEvent),
    Workflow(WorkflowEvent),
    Terminal(TerminalEvent),
    System(SystemEvent),
    Language(LanguageEvent),  // NEW
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum LanguageEvent {
    FileIndexed { file_path: String, symbol_count: usize },
    SymbolsFound { intent_id: Uuid, symbols: Vec<Symbol> },
    CompletionsReady { intent_id: Uuid, items: Vec<CompletionItem> },
    DiagnosticsUpdated { file_path: String, diagnostics: Vec<Diagnostic> },
}
```

**3.3 Implement Language Intent Handler**
```rust
// src-tauri/src/language/handler.rs

pub struct LanguageHandler {
    tree_sitter: TreeSitterParser,
    lsp_manager: LspManager,
}

impl LanguageHandler {
    pub async fn handle_intent(
        &mut self,
        intent: LanguageIntent,
        intent_id: Uuid,
        app: AppHandle,
    ) -> Result<(), Error> {
        match intent {
            LanguageIntent::IndexFile { file_path } => {
                let content = fs::read_to_string(&file_path)?;
                let language = detect_language(&file_path)?;
                
                // Parse with tree-sitter
                let tree = self.tree_sitter.parse(&content, language)?;
                
                // Extract symbols
                let symbols = extract_symbols(&tree, &content, language);
                
                // Store in index (TODO: Week 4)
                
                // Emit event
                app.emit("language-event", LanguageEvent::FileIndexed {
                    file_path,
                    symbol_count: symbols.len(),
                })?;
                
                Ok(())
            }
            
            LanguageIntent::GetCompletions { file_path, position } => {
                let items = self.lsp_manager
                    .get_completions(&file_path, position)
                    .await?;
                
                app.emit("language-event", LanguageEvent::CompletionsReady {
                    intent_id,
                    items,
                })?;
                
                Ok(())
            }
            
            // ... other intents
        }
    }
}
```

**3.4 Update Dispatcher**
```rust
// src-tauri/src/commands.rs

#[tauri::command]
pub async fn dispatch(
    envelope: BladeIntentEnvelope,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    match envelope.intent {
        BladeIntent::Language(intent) => {
            state.language_handler.lock().await
                .handle_intent(intent, envelope.id, app)
                .await
                .map_err(|e| e.to_string())
        }
        // ... other domains
    }
}
```

**3.5 Create Frontend Types**
```typescript
// src/types/language.ts

export type LanguageIntent =
  | { type: 'IndexFile'; file_path: string }
  | { type: 'SearchSymbols'; query: string }
  | { type: 'GetCompletions'; file_path: string; position: Position };

export type LanguageEvent =
  | { type: 'FileIndexed'; file_path: string; symbol_count: number }
  | { type: 'SymbolsFound'; intent_id: string; symbols: Symbol[] }
  | { type: 'CompletionsReady'; intent_id: string; items: CompletionItem[] };

export interface Symbol {
  id: string;
  name: string;
  symbol_type: SymbolType;
  file_path: string;
  range: Range;
}
```

**Deliverables:**
- ✅ BCP Language domain defined
- ✅ Intent/Event handlers implemented
- ✅ TypeScript types generated
- ✅ Basic operations working through BCP

---

## Phase 2: Symbol Index (Weeks 4-5)

### Week 4: SQLite Symbol Index

**Goal:** Persistent symbol storage and fast search

#### Tasks

**4.1 Create Database Schema**
```sql
-- .zblade/index/symbols.db

CREATE TABLE symbols (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    symbol_type TEXT NOT NULL,
    file_path TEXT NOT NULL,
    start_line INTEGER NOT NULL,
    start_char INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    end_char INTEGER NOT NULL,
    parent_id TEXT,
    docstring TEXT,
    signature TEXT,
    indexed_at INTEGER NOT NULL
);

CREATE INDEX idx_symbols_name ON symbols(name);
CREATE INDEX idx_symbols_file ON symbols(file_path);
CREATE INDEX idx_symbols_type ON symbols(symbol_type);
CREATE INDEX idx_symbols_parent ON symbols(parent_id);

CREATE VIRTUAL TABLE symbols_fts USING fts5(
    name,
    docstring,
    content=symbols,
    content_rowid=rowid
);
```

**4.2 Implement Symbol Index**
```rust
// src-tauri/src/symbol_index/mod.rs

pub struct SymbolIndex {
    db: Connection,
}

impl SymbolIndex {
    pub fn new(db_path: &Path) -> Result<Self, Error> {
        let db = Connection::open(db_path)?;
        Self::create_schema(&db)?;
        Ok(Self { db })
    }
    
    pub fn insert_symbols(&self, symbols: &[Symbol]) -> Result<(), Error> {
        let tx = self.db.transaction()?;
        
        for symbol in symbols {
            tx.execute(
                "INSERT OR REPLACE INTO symbols VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    symbol.id,
                    symbol.name,
                    symbol.symbol_type.to_string(),
                    symbol.file_path,
                    symbol.range.start.line,
                    symbol.range.start.character,
                    symbol.range.end.line,
                    symbol.range.end.character,
                    symbol.parent_id,
                    symbol.docstring,
                    symbol.signature,
                    Utc::now().timestamp(),
                ],
            )?;
        }
        
        tx.commit()?;
        Ok(())
    }
    
    pub fn search(&self, query: &str) -> Result<Vec<Symbol>, Error> {
        let mut stmt = self.db.prepare(
            "SELECT * FROM symbols 
             WHERE name LIKE ?1 
             ORDER BY name 
             LIMIT 50"
        )?;
        
        let symbols = stmt.query_map([format!("%{}%", query)], |row| {
            Ok(Symbol {
                id: row.get(0)?,
                name: row.get(1)?,
                symbol_type: row.get::<_, String>(2)?.parse().unwrap(),
                file_path: row.get(3)?,
                range: Range {
                    start: Position {
                        line: row.get(4)?,
                        character: row.get(5)?,
                    },
                    end: Position {
                        line: row.get(6)?,
                        character: row.get(7)?,
                    },
                },
                parent_id: row.get(8)?,
                docstring: row.get(9)?,
                signature: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
        
        Ok(symbols)
    }
    
    pub fn get_symbols_in_file(&self, file_path: &str) -> Result<Vec<Symbol>, Error> {
        // Similar to search, but filter by file_path
    }
    
    pub fn delete_file_symbols(&self, file_path: &str) -> Result<(), Error> {
        self.db.execute(
            "DELETE FROM symbols WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(())
    }
}
```

**4.3 Integrate with Tree-sitter**
```rust
impl LanguageHandler {
    pub async fn index_file(&mut self, file_path: &str) -> Result<(), Error> {
        // Parse file
        let content = fs::read_to_string(file_path)?;
        let language = detect_language(file_path)?;
        let tree = self.tree_sitter.parse(&content, language)?;
        
        // Extract symbols
        let symbols = extract_symbols(&tree, &content, language);
        
        // Store in index
        self.symbol_index.insert_symbols(&symbols)?;
        
        Ok(())
    }
    
    pub async fn search_symbols(&self, query: &str) -> Result<Vec<Symbol>, Error> {
        self.symbol_index.search(query)
    }
}
```

**Deliverables:**
- ✅ SQLite symbol index working
- ✅ Fast search (<50ms)
- ✅ File indexing integrated
- ✅ Incremental updates working

---

### Week 5: Unified Symbol Index

**Goal:** Merge tree-sitter and LSP symbol data

#### Tasks

**5.1 Implement Unified Symbol**
```rust
// src-tauri/src/unified_index/mod.rs

pub struct UnifiedSymbol {
    // Core (from tree-sitter)
    pub id: String,
    pub name: String,
    pub symbol_type: SymbolType,
    pub file_path: String,
    pub range: Range,
    
    // Syntax (from tree-sitter)
    pub parent_id: Option<String>,
    pub children: Vec<String>,
    pub docstring: Option<String>,
    
    // Semantics (from LSP)
    pub type_signature: Option<String>,
    pub documentation: Option<String>,
    pub references: Option<Vec<Location>>,
    
    // Metadata
    pub source: SymbolSource,
}

pub enum SymbolSource {
    TreeSitter,
    Lsp,
    Both,
}
```

**5.2 Implement Enrichment Pipeline**
```rust
pub struct UnifiedIndex {
    tree_sitter_index: SymbolIndex,
    lsp_manager: LspManager,
    enrichment_cache: LruCache<String, UnifiedSymbol>,
}

impl UnifiedIndex {
    pub async fn get_enriched_symbol(&mut self, symbol_id: &str) -> Result<UnifiedSymbol, Error> {
        // Check cache
        if let Some(cached) = self.enrichment_cache.get(symbol_id) {
            return Ok(cached.clone());
        }
        
        // Get base symbol from tree-sitter
        let base = self.tree_sitter_index.get_symbol(symbol_id)?;
        
        // Enrich with LSP data
        let enriched = self.enrich_with_lsp(base).await?;
        
        // Cache result
        self.enrichment_cache.put(symbol_id.to_string(), enriched.clone());
        
        Ok(enriched)
    }
    
    async fn enrich_with_lsp(&mut self, symbol: Symbol) -> Result<UnifiedSymbol, Error> {
        // Get hover info (type + docs)
        let hover = self.lsp_manager
            .get_hover(&symbol.file_path, symbol.range.start)
            .await
            .ok();
        
        Ok(UnifiedSymbol {
            id: symbol.id,
            name: symbol.name,
            symbol_type: symbol.symbol_type,
            file_path: symbol.file_path,
            range: symbol.range,
            parent_id: symbol.parent_id,
            children: vec![],
            docstring: symbol.docstring,
            type_signature: hover.as_ref().and_then(|h| h.type_signature.clone()),
            documentation: hover.and_then(|h| h.documentation),
            references: None,  // Expensive, load on demand
            source: if hover.is_some() { 
                SymbolSource::Both 
            } else { 
                SymbolSource::TreeSitter 
            },
        })
    }
}
```

**Deliverables:**
- ✅ Unified symbol model
- ✅ Enrichment pipeline working
- ✅ Cache optimization
- ✅ Performance targets met

---

## Phase 3: RFC-002 Integration (Week 6)

### Week 6: Semantic Code References

**Goal:** Replace line-based references with symbol-based

#### Tasks

**6.1 Update Conversation Storage**
```rust
// Update conversation artifact format

#[derive(Serialize, Deserialize)]
pub struct SemanticCodeReference {
    pub symbol_id: String,           // NEW: stable identifier
    pub symbol_name: String,          // NEW: for display
    pub file_path: String,
    pub range: Range,                 // Current location (may change)
    pub context: String,
}

// Old format (deprecated)
#[derive(Serialize, Deserialize)]
pub struct LineBasedReference {
    pub file: String,
    pub lines: [usize; 2],
    pub context: String,
}
```

**6.2 Implement Context Assembly**
```rust
impl LanguageHandler {
    pub async fn assemble_context(
        &mut self,
        references: Vec<SemanticCodeReference>,
        max_tokens: usize,
    ) -> Result<AssembledContext, Error> {
        let mut parts = Vec::new();
        
        for ref in references {
            // Get current symbol (may have moved)
            let symbol = self.unified_index
                .get_enriched_symbol(&ref.symbol_id)
                .await?;
            
            // Extract minimal context
            let code = self.extract_symbol_code(&symbol)?;
            
            parts.push(ContextPart {
                symbol_name: symbol.name,
                code,
                type_signature: symbol.type_signature,
                tokens: estimate_tokens(&code),
            });
        }
        
        // Compress to fit budget
        self.compress_context(parts, max_tokens)
    }
    
    fn extract_symbol_code(&self, symbol: &UnifiedSymbol) -> Result<String, Error> {
        let content = fs::read_to_string(&symbol.file_path)?;
        let lines: Vec<&str> = content.lines().collect();
        
        // Extract just the symbol's code
        let start = symbol.range.start.line as usize;
        let end = symbol.range.end.line as usize;
        
        Ok(lines[start..=end].join("\n"))
    }
}
```

**6.3 Update AI Context Retrieval**
```rust
// When AI needs context, use semantic references

pub async fn retrieve_context_for_ai(
    conversation_id: &str,
    language_handler: &mut LanguageHandler,
) -> Result<String, Error> {
    // Get semantic references from conversation
    let references = get_conversation_references(conversation_id)?;
    
    // Assemble context using symbols
    let context = language_handler
        .assemble_context(references, MAX_CONTEXT_TOKENS)
        .await?;
    
    Ok(context.to_string())
}
```

**6.4 Measure Token Reduction**
```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_token_reduction() {
        let old_context = assemble_line_based_context(&refs);
        let new_context = assemble_semantic_context(&refs).await;
        
        let old_tokens = count_tokens(&old_context);
        let new_tokens = count_tokens(&new_context);
        
        let reduction = (old_tokens - new_tokens) as f64 / old_tokens as f64;
        
        assert!(reduction >= 0.40, "Expected 40%+ reduction, got {:.1}%", reduction * 100.0);
    }
}
```

**Deliverables:**
- ✅ Semantic references working
- ✅ Context assembly optimized
- ✅ 40%+ token reduction validated
- ✅ RFC-002 integration complete

---

## Phase 4: AI Self-Correction (Weeks 7-8)

### Week 7: Semantic Patch Application

**Goal:** AI can apply patches at AST level with validation

#### Tasks

**7.1 Implement Semantic Patch Format**
```rust
#[derive(Serialize, Deserialize)]
pub struct SemanticPatch {
    pub target: PatchTarget,
    pub operation: PatchOperation,
    pub new_code: String,
}

pub enum PatchTarget {
    Symbol { symbol_id: String },
    Range { file_path: String, range: Range },
}

pub enum PatchOperation {
    Replace,
    InsertBefore,
    InsertAfter,
    Delete,
}
```

**7.2 Implement Patch Applier**
```rust
impl LanguageHandler {
    pub async fn apply_semantic_patch(
        &mut self,
        patch: &SemanticPatch,
    ) -> Result<PatchResult, Error> {
        // 1. Find target
        let (file_path, range) = self.resolve_patch_target(&patch.target)?;
        
        // 2. Validate new code syntax (tree-sitter)
        self.validate_syntax(&patch.new_code)?;
        
        // 3. Read original content
        let original = fs::read_to_string(&file_path)?;
        
        // 4. Apply patch
        let new_content = self.apply_patch_to_content(&original, &range, &patch)?;
        
        // 5. Write to file
        fs::write(&file_path, &new_content)?;
        
        // 6. Wait for LSP diagnostics
        tokio::time::sleep(Duration::from_millis(500)).await;
        let diagnostics = self.lsp_manager.get_diagnostics(&file_path).await?;
        
        // 7. Check for new errors
        if self.has_new_errors(&diagnostics) {
            // Rollback
            fs::write(&file_path, &original)?;
            return Err(Error::PatchIntroducedErrors { diagnostics });
        }
        
        // 8. Re-index file
        self.index_file(&file_path).await?;
        
        Ok(PatchResult {
            success: true,
            file_path,
            validated_by_lsp: true,
        })
    }
}
```

**Deliverables:**
- ✅ Semantic patch application working
- ✅ LSP validation integrated
- ✅ Automatic rollback on errors
- ✅ 80%+ success rate

---

### Week 8: AI Self-Correction Loop

**Goal:** AI automatically fixes errors using LSP feedback

#### Tasks

**8.1 Implement Diagnostic Analyzer**
```rust
pub struct DiagnosticAnalyzer {
    fixable_error_codes: HashSet<String>,
}

impl DiagnosticAnalyzer {
    pub fn should_auto_fix(&self, diagnostic: &Diagnostic) -> bool {
        diagnostic.severity == DiagnosticSeverity::Error
            && self.fixable_error_codes.contains(&diagnostic.code)
    }
    
    pub fn is_fixable_error(&self, code: &str) -> bool {
        matches!(code,
            "TS2304" |  // Cannot find name
            "TS2305" |  // Module has no exported member
            "TS2339" |  // Property does not exist
            "TS2345" |  // Argument of type X is not assignable to Y
            // ... more fixable errors
        )
    }
}
```

**8.2 Implement AI Fix Request**
```rust
pub async fn request_ai_fix(
    diagnostic: &Diagnostic,
    file_path: &str,
    tree_sitter: &TreeSitterParser,
    ai_client: &AiClient,
) -> Result<SemanticPatch, Error> {
    // Extract context around error
    let context = tree_sitter.extract_context_around(
        file_path,
        diagnostic.range,
        ContextSize::Medium,
    )?;
    
    // Build fix request
    let prompt = format!(
        "Fix this TypeScript error:\n\
         Error: {}\n\
         Code: {}\n\n\
         Context:\n{}\n\n\
         Provide only the corrected code.",
        diagnostic.message,
        diagnostic.code,
        context
    );
    
    // Request fix from AI
    let fixed_code = ai_client.request_completion(prompt).await?;
    
    // Create semantic patch
    Ok(SemanticPatch {
        target: PatchTarget::Range {
            file_path: file_path.to_string(),
            range: diagnostic.range,
        },
        operation: PatchOperation::Replace,
        new_code: fixed_code,
    })
}
```

**8.3 Implement Self-Correction Loop**
```rust
pub async fn handle_diagnostic_with_auto_fix(
    diagnostic: Diagnostic,
    file_path: String,
    state: &AppState,
) -> Result<(), Error> {
    // Check if fixable
    if !state.diagnostic_analyzer.should_auto_fix(&diagnostic) {
        return Ok(());  // Skip non-fixable errors
    }
    
    const MAX_ATTEMPTS: usize = 3;
    let mut attempt = 0;
    
    while attempt < MAX_ATTEMPTS {
        attempt += 1;
        
        // Request AI fix
        let patch = request_ai_fix(
            &diagnostic,
            &file_path,
            &state.tree_sitter,
            &state.ai_client,
        ).await?;
        
        // Apply patch
        match state.language_handler.apply_semantic_patch(&patch).await {
            Ok(_) => {
                // Success! Emit event
                state.app.emit("language-event", LanguageEvent::AiFixApplied {
                    file_path,
                    diagnostic_message: diagnostic.message,
                })?;
                return Ok(());
            }
            Err(Error::PatchIntroducedErrors { diagnostics }) => {
                // Patch failed, try again with error feedback
                if attempt < MAX_ATTEMPTS {
                    // Feed errors back to AI for next attempt
                    continue;
                } else {
                    // Give up after max attempts
                    return Err(Error::AiFixFailed {
                        attempts: MAX_ATTEMPTS,
                        last_errors: diagnostics,
                    });
                }
            }
            Err(e) => return Err(e),
        }
    }
    
    Ok(())
}
```

**Deliverables:**
- ✅ AI self-correction working
- ✅ Multi-attempt loop with feedback
- ✅ Success metrics tracked
- ✅ User notifications

---

## Phase 5: Editor Features (Weeks 9-10)

### Week 9: Frontend Integration

**Goal:** CodeMirror extensions using backend services

#### Tasks

**9.1 Create Language Hook**
```typescript
// src/hooks/useLanguage.ts

export function useLanguage() {
  const [symbols, setSymbols] = useState<UnifiedSymbol[]>([]);
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
  const [completions, setCompletions] = useState<CompletionItem[]>([]);
  
  useEffect(() => {
    const unlisten = listen<LanguageEvent>('language-event', (event) => {
      switch (event.payload.type) {
        case 'SymbolsFound':
          setSymbols(event.payload.symbols);
          break;
        case 'DiagnosticsUpdated':
          setDiagnostics(event.payload.diagnostics);
          break;
        case 'CompletionsReady':
          setCompletions(event.payload.items);
          break;
      }
    });
    
    return () => { unlisten.then(fn => fn()); };
  }, []);
  
  const searchSymbols = useCallback(async (query: string) => {
    await invoke('dispatch', {
      id: uuidv4(),
      timestamp: Date.now(),
      intent: {
        type: 'Language',
        payload: { type: 'SearchSymbols', query }
      }
    });
  }, []);
  
  return { symbols, diagnostics, completions, searchSymbols };
}
```

**9.2 Create CodeMirror Extensions**
```typescript
// src/components/editor/extensions/diagnostics.ts

export function createDiagnosticsExtension() {
  return [
    diagnosticsField,
    diagnosticGutter,
    diagnosticDecorations,
  ];
}

const diagnosticsField = StateField.define<Diagnostic[]>({
  create() { return []; },
  update(diagnostics, tr) {
    for (let effect of tr.effects) {
      if (effect.is(setDiagnostics)) {
        return effect.value;
      }
    }
    return diagnostics;
  }
});

const diagnosticGutter = gutter({
  class: 'cm-diagnostic-gutter',
  markers: (view) => {
    const diagnostics = view.state.field(diagnosticsField);
    // Render error/warning icons
  }
});
```

**9.3 Integrate with CodeEditor**
```typescript
// src/components/CodeEditor.tsx

export function CodeEditor({ filePath }: Props) {
  const { diagnostics, searchSymbols } = useLanguage();
  
  const extensions = useMemo(() => [
    basicSetup,
    javascript({ typescript: true }),
    createDiagnosticsExtension(),
    createCompletionExtension(),
    createHoverExtension(),
  ], []);
  
  // Update diagnostics when they change
  useEffect(() => {
    if (editorView) {
      editorView.dispatch({
        effects: setDiagnostics.of(diagnostics)
      });
    }
  }, [diagnostics, editorView]);
  
  return <div ref={editorRef} />;
}
```

**Deliverables:**
- ✅ Frontend hooks working
- ✅ CodeMirror extensions integrated
- ✅ Diagnostics displaying
- ✅ Completions working

---

### Week 10: Polish & Optimization

**Goal:** Performance tuning and user experience refinement

#### Tasks

**10.1 Performance Optimization**
- Profile hot paths
- Optimize symbol search (<50ms)
- Optimize context assembly (<100ms)
- Reduce memory usage
- Add more caching

**10.2 User Experience**
- Smooth animations
- Loading states
- Error messages
- Progress indicators
- Keyboard shortcuts

**10.3 Documentation**
- User guide
- API documentation
- Architecture diagrams
- Performance benchmarks

**10.4 Testing**
- End-to-end tests
- Performance tests
- Stress tests
- User acceptance testing

**Deliverables:**
- ✅ Performance targets met
- ✅ Smooth user experience
- ✅ Complete documentation
- ✅ Production-ready

---

## Success Metrics

### Performance Targets

| Metric | Target | Validation |
|--------|--------|------------|
| Symbol search | <50ms | Benchmark |
| Context assembly | <100ms | Benchmark |
| Patch application | <500ms | Benchmark |
| Workspace index | <20s (1000 files) | Integration test |
| Token reduction | 40-60% | A/B test |
| Patch success rate | >80% | Production metrics |

### Feature Completeness

- ✅ Tree-sitter parsing (TypeScript, JavaScript, Rust, Python)
- ✅ Symbol extraction and indexing
- ✅ LSP integration (diagnostics, completions, hover)
- ✅ Semantic code references (RFC-002)
- ✅ AI context optimization
- ✅ Semantic patch application
- ✅ AI self-correction loop
- ✅ CodeMirror integration

---

## Risk Mitigation

### Technical Risks

**Risk 1: Performance doesn't meet targets**
- Mitigation: Profile early, optimize incrementally
- Fallback: Reduce scope (fewer languages)

**Risk 2: LSP integration complexity**
- Mitigation: Start with one language server
- Fallback: Use tree-sitter only for MVP

**Risk 3: Token reduction less than expected**
- Mitigation: Measure early, adjust extraction strategy
- Fallback: Still valuable for reference stability

### Schedule Risks

**Risk 1: Underestimated complexity**
- Mitigation: Weekly validation gates
- Fallback: Ship phases incrementally

**Risk 2: Blocked by dependencies**
- Mitigation: Parallel work streams
- Fallback: Mock dependencies for testing

---

## Next Steps

1. **Review this plan** - Ensure alignment with vision
2. **Set up development environment** - Install dependencies
3. **Create feature branch** - `feature/lsp-tree-sitter-integration`
4. **Start Week 1** - Tree-sitter backend setup
5. **Daily standups** - Track progress, adjust plan

---

**Document Version**: 1.0  
**Last Updated**: 2026-01-18  
**Status**: Active - Ready for Implementation
