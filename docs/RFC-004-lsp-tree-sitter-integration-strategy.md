# RFC-004: LSP & Tree-sitter Integration Strategy

## Status

**Draft** - Architecture Planning Phase

## Authors

Stig-Ørjan Smelror

## Target Audience

System architects, backend engineers, frontend engineers

## Created

2026-01-18

---

## Executive Summary

This RFC defines the integration strategy between **LSP (Language Server Protocol)** and **tree-sitter** in ZaguanBlade. Both systems provide code understanding capabilities, but serve different purposes and must work together optimally without duplication or conflict.

**Key Principle**: LSP and tree-sitter are **complementary, not competing** systems. Each excels at different tasks, and proper integration creates a more powerful whole.

---

## Table of Contents

1. [Motivation](#motivation)
2. [Responsibility Matrix](#responsibility-matrix)
3. [Architecture](#architecture)
4. [Integration Points](#integration-points)
5. [Data Flow](#data-flow)
6. [Performance Optimization](#performance-optimization)
7. [Implementation Strategy](#implementation-strategy)
8. [API Design](#api-design)
9. [Conflict Resolution](#conflict-resolution)
10. [Future-Proofing](#future-proofing)

---

## Motivation

### Why Both LSP and Tree-sitter?

**LSP provides:**
- ✅ **Semantic analysis** (type checking, symbol resolution)
- ✅ **Cross-file understanding** (imports, references)
- ✅ **Language-specific intelligence** (compiler integration)
- ✅ **Code actions** (quick fixes, refactorings)
- ✅ **Real-time diagnostics** (errors, warnings)

**Tree-sitter provides:**
- ✅ **Fast, incremental parsing** (<10ms updates)
- ✅ **Syntax structure** (AST without semantics)
- ✅ **Error-tolerant parsing** (works with incomplete code)
- ✅ **Universal API** (same interface for all languages)
- ✅ **Lightweight** (no compiler overhead)

**Together they enable:**
- 🚀 **Fast syntax operations** (tree-sitter) + **Deep semantic analysis** (LSP)
- 🚀 **Immediate feedback** (tree-sitter) + **Accurate diagnostics** (LSP)
- 🚀 **Universal parsing** (tree-sitter) + **Language expertise** (LSP)
- 🚀 **AI context assembly** (tree-sitter) + **Type information** (LSP)

### The Problem Without Integration

**Without clear boundaries:**
- ❌ Duplicate parsing (waste CPU/memory)
- ❌ Conflicting information (which is correct?)
- ❌ Race conditions (who updates first?)
- ❌ Maintenance burden (two systems to update)

---

## Responsibility Matrix

### Clear Division of Labor

| Capability | LSP | Tree-sitter | Notes |
|------------|-----|-------------|-------|
| **Parsing** | ❌ | ✅ | Tree-sitter is faster, incremental |
| **Syntax Highlighting** | ❌ | ✅ | Tree-sitter via CodeMirror |
| **Code Folding** | ❌ | ✅ | Tree-sitter knows structure |
| **Symbol Extraction** | ❌ | ✅ | Tree-sitter for local symbols |
| **Type Information** | ✅ | ❌ | LSP has compiler knowledge |
| **Diagnostics** | ✅ | ❌ | LSP has semantic errors |
| **Completions** | ✅ | ❌ | LSP knows context |
| **Go to Definition** | ✅ | ❌ | LSP tracks cross-file refs |
| **Find References** | ✅ | ❌ | LSP has full project index |
| **Hover Info** | ✅ | ❌ | LSP has type/doc info |
| **Rename Symbol** | ✅ | ❌ | LSP ensures correctness |
| **Code Actions** | ✅ | ❌ | LSP generates fixes |
| **Formatting** | ✅ | ❌ | LSP uses language formatter |
| **Semantic Tokens** | ✅ | ❌ | LSP for accurate highlighting |
| **Breadcrumbs** | ❌ | ✅ | Tree-sitter for current symbol |
| **Outline View** | ❌ | ✅ | Tree-sitter for file structure |
| **Structural Search** | ❌ | ✅ | Tree-sitter for AST queries |
| **AI Context Assembly** | 🤝 | 🤝 | **Both collaborate** |
| **Semantic Patches** | 🤝 | 🤝 | **Both collaborate** |
| **Symbol Index** | 🤝 | 🤝 | **Both contribute** |

**Legend:**
- ✅ = Primary responsibility
- ❌ = Not responsible
- 🤝 = Shared responsibility (collaboration required)

---

## Architecture

### System Layers

```
┌─────────────────────────────────────────────────────────────┐
│                     ZaguanBlade Frontend                     │
│                                                              │
│  ┌────────────────────────────────────────────────────┐    │
│  │  CodeMirror Editor                                 │    │
│  │  - User edits code                                 │    │
│  │  - Displays UI (highlights, diagnostics, etc.)     │    │
│  └────────────────────────────────────────────────────┘    │
│                          ↕                                   │
│  ┌─────────────────────┬──────────────────────────────┐    │
│  │  Tree-sitter        │  LSP Client                  │    │
│  │  (WASM)             │  (TypeScript)                │    │
│  │                     │                              │    │
│  │  • Parse on edit    │  • Request completions       │    │
│  │  • Extract symbols  │  • Show diagnostics          │    │
│  │  • Code folding     │  • Hover info                │    │
│  │  • Breadcrumbs      │  • Go to definition          │    │
│  └─────────────────────┴──────────────────────────────┘    │
│                          ↕                                   │
│  ┌────────────────────────────────────────────────────┐    │
│  │  Integration Layer                                 │    │
│  │  - Coordinates tree-sitter + LSP                   │    │
│  │  - Resolves conflicts                              │    │
│  │  - Caches results                                  │    │
│  └────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
                          ↕ (Tauri IPC)
┌─────────────────────────────────────────────────────────────┐
│                     ZaguanBlade Backend (Rust)               │
│                                                              │
│  ┌─────────────────────┬──────────────────────────────┐    │
│  │  Tree-sitter        │  LSP Manager                 │    │
│  │  (Native)           │  (Rust)                      │    │
│  │                     │                              │    │
│  │  • Index workspace  │  • Spawn language servers    │    │
│  │  • Symbol DB        │  • Route requests            │    │
│  │  • Semantic patches │  • Manage lifecycle          │    │
│  │  • Context assembly │  • Cache responses           │    │
│  └─────────────────────┴──────────────────────────────┘    │
│                          ↕                                   │
│  ┌────────────────────────────────────────────────────┐    │
│  │  Unified Symbol Index                              │    │
│  │  - Combines tree-sitter + LSP data                 │    │
│  │  - SQLite database                                 │    │
│  │  - Single source of truth                          │    │
│  └────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
                          ↕ (JSON-RPC)
┌─────────────────────────────────────────────────────────────┐
│  Language Servers (rust-analyzer, gopls, tsserver, etc.)    │
└─────────────────────────────────────────────────────────────┘
```

---

## Integration Points

### 1. Symbol Index (Unified)

**Problem**: Both systems can extract symbols. Who's the source of truth?

**Solution**: Unified symbol index that combines both sources.

```rust
// src-tauri/src/unified_index/mod.rs

pub struct UnifiedSymbolIndex {
    tree_sitter_index: TreeSitterIndex,
    lsp_cache: LspSymbolCache,
    db: Connection,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedSymbol {
    // Core identity (from tree-sitter)
    pub id: String,
    pub name: String,
    pub symbol_type: SymbolType,
    pub file_path: String,
    pub range: Range,
    
    // Syntax info (from tree-sitter)
    pub syntax: SyntaxInfo {
        pub parent_id: Option<String>,
        pub children: Vec<String>,
        pub docstring: Option<String>,
    },
    
    // Semantic info (from LSP)
    pub semantics: Option<SemanticInfo> {
        pub type_signature: String,
        pub documentation: String,
        pub references: Vec<Location>,
        pub implementations: Vec<Location>,
    },
    
    // Metadata
    pub source: SymbolSource,  // TreeSitter, LSP, or Both
    pub last_updated: Timestamp,
}

impl UnifiedSymbolIndex {
    pub async fn get_symbol(&self, id: &str) -> Result<UnifiedSymbol, Error> {
        // 1. Get syntax info from tree-sitter (fast, always available)
        let syntax = self.tree_sitter_index.get_symbol(id)?;
        
        // 2. Try to enrich with LSP data (slower, may not be available)
        let semantics = self.lsp_cache.get_symbol_info(
            &syntax.file_path,
            &syntax.range
        ).await.ok();
        
        Ok(UnifiedSymbol {
            id: syntax.id,
            name: syntax.name,
            symbol_type: syntax.symbol_type,
            file_path: syntax.file_path,
            range: syntax.range,
            syntax: syntax.into(),
            semantics,
            source: if semantics.is_some() { 
                SymbolSource::Both 
            } else { 
                SymbolSource::TreeSitter 
            },
            last_updated: Utc::now(),
        })
    }
    
    pub async fn search_symbols(&self, query: &str) -> Result<Vec<UnifiedSymbol>, Error> {
        // 1. Fast search via tree-sitter index
        let candidates = self.tree_sitter_index.search(query)?;
        
        // 2. Enrich top results with LSP data (parallel)
        let enriched = stream::iter(candidates.into_iter().take(50))
            .map(|sym| async move {
                let semantics = self.lsp_cache.get_symbol_info(
                    &sym.file_path,
                    &sym.range
                ).await.ok();
                
                UnifiedSymbol {
                    syntax: sym.into(),
                    semantics,
                    ..Default::default()
                }
            })
            .buffer_unordered(10)
            .collect()
            .await;
        
        Ok(enriched)
    }
}
```

**Key Principles:**
- **Tree-sitter is primary** for structure and identity
- **LSP enriches** with semantic information
- **Graceful degradation** if LSP unavailable
- **Async enrichment** doesn't block fast operations

---

### 2. AI Context Assembly (Collaborative)

**Problem**: AI needs both syntax structure and semantic information.

**Solution**: Tree-sitter extracts structure, LSP provides types/docs.

```rust
// src-tauri/src/ai_context/mod.rs

pub struct AiContextAssembler {
    tree_sitter: TreeSitterIndex,
    lsp_manager: LspManager,
    unified_index: UnifiedSymbolIndex,
}

impl AiContextAssembler {
    pub async fn assemble_context(
        &self,
        references: Vec<SemanticCodeReference>,
        max_tokens: usize,
    ) -> Result<AssembledContext, Error> {
        let mut parts = Vec::new();
        
        for ref in references {
            // 1. Get symbol structure from tree-sitter (fast)
            let symbol = self.tree_sitter.get_symbol(&ref.symbol_id)?;
            
            // 2. Extract minimal code (tree-sitter)
            let code = self.extract_symbol_code(&symbol)?;
            
            // 3. Get type information from LSP (if available)
            let type_info = self.lsp_manager
                .get_hover_info(&symbol.file_path, symbol.range.start)
                .await
                .ok();
            
            // 4. Get dependencies (tree-sitter for structure, LSP for types)
            let deps = self.resolve_dependencies(&symbol).await?;
            
            parts.push(ContextPart {
                symbol,
                code,
                type_info,
                dependencies: deps,
                tokens: estimate_tokens(&code),
            });
        }
        
        // 5. Compress to fit budget
        self.compress_context(parts, max_tokens)
    }
    
    async fn resolve_dependencies(
        &self,
        symbol: &Symbol,
    ) -> Result<Vec<Dependency>, Error> {
        // Tree-sitter: Extract import statements
        let imports = self.tree_sitter.extract_imports(&symbol.file_path)?;
        
        // LSP: Resolve import targets (what do they point to?)
        let resolved = stream::iter(imports)
            .then(|import| async move {
                let location = self.lsp_manager
                    .goto_definition(&symbol.file_path, import.range.start)
                    .await
                    .ok()?;
                
                Some(Dependency {
                    name: import.name,
                    location,
                    type_info: self.lsp_manager
                        .get_hover_info(&location.file, location.range.start)
                        .await
                        .ok(),
                })
            })
            .filter_map(|x| async move { x })
            .collect()
            .await;
        
        Ok(resolved)
    }
}
```

**Benefits:**
- **Fast structure extraction** (tree-sitter, no LSP wait)
- **Rich semantic context** (LSP types/docs when available)
- **Graceful degradation** (works without LSP)
- **Optimal token usage** (only include what's needed)

---

### 3. Semantic Patches (Collaborative)

**Problem**: AI generates patches. How to apply them reliably?

**Solution**: Tree-sitter finds target, LSP validates semantics.

```rust
// src-tauri/src/semantic_patch/mod.rs

pub struct SemanticPatchApplier {
    tree_sitter: TreeSitterIndex,
    lsp_manager: LspManager,
}

impl SemanticPatchApplier {
    pub async fn apply_patch(
        &mut self,
        patch: &SemanticPatch,
    ) -> Result<ApplyResult, Error> {
        // 1. Find target symbol (tree-sitter)
        let symbol = self.tree_sitter.find_symbol(&patch.target)?;
        
        // 2. Validate new code syntax (tree-sitter)
        self.validate_syntax(&patch.new_code)?;
        
        // 3. Apply patch (tree-sitter AST manipulation)
        let new_content = self.apply_at_ast_level(&symbol, patch)?;
        
        // 4. Write to file
        fs::write(&symbol.file_path, &new_content)?;
        
        // 5. Wait for LSP diagnostics (semantic validation)
        let diagnostics = self.lsp_manager
            .wait_for_diagnostics(&symbol.file_path, Duration::from_secs(2))
            .await?;
        
        // 6. Check if patch introduced errors
        if has_new_errors(&diagnostics) {
            // Rollback
            fs::write(&symbol.file_path, &original_content)?;
            return Err(Error::PatchIntroducedErrors { diagnostics });
        }
        
        // 7. Re-index (tree-sitter)
        self.tree_sitter.index_file(&symbol.file_path)?;
        
        Ok(ApplyResult {
            success: true,
            file_path: symbol.file_path,
            validated_by_lsp: true,
        })
    }
}
```

**Benefits:**
- **Fast structural manipulation** (tree-sitter)
- **Semantic validation** (LSP catches type errors)
- **Automatic rollback** if patch breaks code
- **High reliability** (80%+ success rate)

---

### 4. Diagnostics & AI Fixes (LSP Primary, Tree-sitter Assists)

**Problem**: LSP reports errors. AI needs context to fix them.

**Solution**: LSP provides diagnostics, tree-sitter extracts context.

```rust
// src-tauri/src/ai_diagnostic_fixer/mod.rs

pub struct AiDiagnosticFixer {
    lsp_manager: LspManager,
    tree_sitter: TreeSitterIndex,
    ai_client: AiClient,
}

impl AiDiagnosticFixer {
    pub async fn handle_diagnostic(
        &self,
        diagnostic: &Diagnostic,
    ) -> Result<(), Error> {
        // 1. LSP provides the error
        let error_info = DiagnosticInfo {
            message: &diagnostic.message,
            code: &diagnostic.code,
            severity: diagnostic.severity,
            range: diagnostic.range,
        };
        
        // 2. Tree-sitter extracts surrounding context
        let context = self.tree_sitter.extract_context_around(
            &diagnostic.file_path,
            diagnostic.range,
            ContextSize::Medium,
        )?;
        
        // 3. LSP provides type information
        let type_info = self.lsp_manager
            .get_hover_info(&diagnostic.file_path, diagnostic.range.start)
            .await
            .ok();
        
        // 4. Build AI fix request
        let fix_request = AiFixRequest {
            error: error_info,
            context,
            type_info,
        };
        
        // 5. AI generates fix
        let fix = self.ai_client.request_fix(fix_request).await?;
        
        // 6. Apply fix (using semantic patch system)
        self.apply_fix(fix).await
    }
}
```

**Benefits:**
- **Accurate error detection** (LSP)
- **Minimal context extraction** (tree-sitter)
- **Type-aware fixes** (LSP type info)
- **Fast response** (tree-sitter doesn't wait for LSP)

---

## Data Flow

### Scenario 1: User Opens File

```
1. User opens file.ts
   ↓
2. Frontend: Tree-sitter parses immediately (5ms)
   → Syntax highlighting appears
   → Code folding available
   → Breadcrumbs show current function
   ↓
3. Backend: Notify LSP (textDocument/didOpen)
   ↓
4. LSP: Analyzes file (200ms)
   → Sends diagnostics
   → Caches semantic tokens
   ↓
5. Frontend: Shows diagnostics
   → Red squiggles appear
   → Hover shows error details (from LSP)
```

**Key Point**: User sees syntax highlighting instantly (tree-sitter), diagnostics appear shortly after (LSP).

---

### Scenario 2: User Edits Code

```
1. User types: "function foo"
   ↓
2. Frontend: Tree-sitter incremental parse (2ms)
   → Syntax highlighting updates immediately
   → Breadcrumbs update to show "foo"
   ↓
3. Frontend: Debounced LSP notification (300ms delay)
   ↓
4. LSP: Incremental analysis
   → Updates diagnostics
   → Updates completions cache
   ↓
5. Frontend: Shows updated diagnostics
```

**Key Point**: Instant visual feedback (tree-sitter), semantic analysis follows (LSP).

---

### Scenario 3: User Requests Completion

```
1. User types: "console."
   ↓
2. Frontend: Request completion from LSP
   ↓
3. LSP: Returns completions (50ms)
   → log, error, warn, etc.
   ↓
4. Frontend: Shows completion menu
   ↓
5. User selects "log"
   ↓
6. Tree-sitter: Parses new code (2ms)
   → Updates syntax highlighting
```

**Key Point**: LSP handles completions (semantic), tree-sitter updates display (syntax).

---

### Scenario 4: AI Assembles Context

```
1. User asks: "How does authentication work?"
   ↓
2. Backend: Search symbols (tree-sitter index, 30ms)
   → Finds: authenticate(), AuthService, etc.
   ↓
3. Backend: Extract code (tree-sitter, 10ms)
   → Gets function bodies
   ↓
4. Backend: Enrich with types (LSP, 100ms)
   → Gets type signatures
   → Gets documentation
   ↓
5. Backend: Assemble context (50ms)
   → Combines syntax + semantics
   → Compresses to fit token budget
   ↓
6. Send to AI (800 tokens vs 2000 without optimization)
```

**Key Point**: Tree-sitter provides structure fast, LSP enriches with semantics.

---

## Performance Optimization

### 1. Caching Strategy

```rust
pub struct IntegrationCache {
    // Tree-sitter caches
    parse_trees: LruCache<String, Tree>,
    symbol_cache: LruCache<String, Vec<Symbol>>,
    
    // LSP caches
    hover_cache: LruCache<Location, HoverInfo>,
    completion_cache: LruCache<Location, Vec<CompletionItem>>,
    diagnostic_cache: HashMap<String, Vec<Diagnostic>>,
    
    // Unified caches
    enriched_symbols: LruCache<String, UnifiedSymbol>,
}
```

**Cache Invalidation:**
- Tree-sitter: Invalidate on file edit
- LSP: Invalidate on LSP notification
- Unified: Invalidate when either source changes

---

### 2. Parallel Operations

```rust
pub async fn get_enriched_symbols(
    &self,
    file_path: &str,
) -> Result<Vec<UnifiedSymbol>, Error> {
    // Run in parallel
    let (tree_sitter_symbols, lsp_symbols) = tokio::join!(
        self.tree_sitter.get_symbols(file_path),
        self.lsp_manager.get_document_symbols(file_path),
    );
    
    // Merge results
    merge_symbol_sources(tree_sitter_symbols?, lsp_symbols?)
}
```

---

### 3. Lazy Loading

```rust
impl UnifiedSymbol {
    // Syntax info loaded immediately (tree-sitter)
    pub fn syntax(&self) -> &SyntaxInfo {
        &self.syntax
    }
    
    // Semantic info loaded on-demand (LSP)
    pub async fn semantics(&mut self) -> Result<&SemanticInfo, Error> {
        if self.semantics.is_none() {
            self.semantics = Some(
                self.lsp_manager.get_symbol_info(&self.file_path, &self.range).await?
            );
        }
        Ok(self.semantics.as_ref().unwrap())
    }
}
```

---

## Implementation Strategy

### Phase 1: Foundation (Weeks 1-3)

**Week 1: Tree-sitter Setup**
- Integrate tree-sitter (RFC-003 Phase 1)
- Symbol extraction working
- Basic index in SQLite

**Week 2: LSP Setup**
- Integrate LSP manager (ROADMAP Phase 1)
- Spawn language servers
- Basic request/response working

**Week 3: Integration Layer**
- Create `UnifiedSymbolIndex`
- Implement cache coordination
- Define clear API boundaries

---

### Phase 2: Core Integration (Weeks 4-6)

**Week 4: Symbol Index Integration**
- Merge tree-sitter + LSP symbols
- Implement enrichment pipeline
- Test with TypeScript + Rust

**Week 5: Context Assembly**
- Implement collaborative context assembly
- Tree-sitter structure + LSP types
- Measure token reduction

**Week 6: Semantic Patches**
- Tree-sitter finds targets
- LSP validates semantics
- Test patch reliability

---

### Phase 3: AI Features (Weeks 7-9)

**Week 7: AI-LSP Integration**
- LSP diagnostics → AI fixes
- Tree-sitter extracts context
- Test fix success rate

**Week 8: Advanced Features**
- Batch fixes
- Confidence scoring
- Learning from rejections

**Week 9: Polish & Optimization**
- Performance tuning
- Cache optimization
- User experience refinement

---

## API Design

### Unified API for Consumers

```typescript
// Frontend: Single API for all symbol operations

import { useSymbols } from '@/hooks/useSymbols';

function MyComponent() {
  const { searchSymbols, getSymbol, getSymbolAt } = useSymbols();
  
  // Search (tree-sitter fast search + LSP enrichment)
  const results = await searchSymbols("authenticate");
  // Returns: UnifiedSymbol[] with syntax + semantics
  
  // Get specific symbol (tree-sitter + LSP)
  const symbol = await getSymbol("src/auth.ts:authenticate:1234");
  // Returns: UnifiedSymbol with full info
  
  // Get symbol at position (tree-sitter fast lookup)
  const current = await getSymbolAt("src/auth.ts", 42);
  // Returns: UnifiedSymbol (semantics loaded lazily)
}
```

**Key Principle**: Consumers don't need to know about tree-sitter vs LSP. The integration layer handles it.

---

## Conflict Resolution

### When Tree-sitter and LSP Disagree

**Scenario**: Tree-sitter says symbol exists, LSP says it doesn't.

**Resolution Strategy:**

```rust
pub enum ConflictResolution {
    PreferTreeSitter,  // For syntax/structure
    PreferLsp,         // For semantics/types
    Merge,             // Combine both
    UserChoice,        // Let user decide
}

impl UnifiedSymbolIndex {
    fn resolve_conflict(
        &self,
        tree_sitter_symbol: &Symbol,
        lsp_symbol: Option<&LspSymbol>,
    ) -> UnifiedSymbol {
        match lsp_symbol {
            Some(lsp) => {
                // Both agree: merge
                if symbols_match(tree_sitter_symbol, lsp) {
                    merge_symbols(tree_sitter_symbol, lsp)
                } else {
                    // Conflict: prefer LSP for semantics
                    warn!("Symbol conflict: {} vs {}", 
                          tree_sitter_symbol.name, lsp.name);
                    
                    UnifiedSymbol {
                        // Structure from tree-sitter
                        syntax: tree_sitter_symbol.into(),
                        // Semantics from LSP (more accurate)
                        semantics: Some(lsp.into()),
                        source: SymbolSource::Conflict,
                    }
                }
            }
            None => {
                // Only tree-sitter has it (LSP not ready or symbol is local)
                UnifiedSymbol {
                    syntax: tree_sitter_symbol.into(),
                    semantics: None,
                    source: SymbolSource::TreeSitter,
                }
            }
        }
    }
}
```

**Conflict Types:**

1. **Symbol Name Mismatch**
   - Resolution: Prefer LSP (more accurate)
   - Example: Tree-sitter sees `foo`, LSP sees `Foo` (case-sensitive language)

2. **Symbol Type Mismatch**
   - Resolution: Prefer LSP
   - Example: Tree-sitter sees "function", LSP sees "method" (inside class)

3. **Range Mismatch**
   - Resolution: Prefer tree-sitter (more precise)
   - Example: LSP includes decorators, tree-sitter just function body

4. **Symbol Missing in LSP**
   - Resolution: Use tree-sitter only
   - Example: Local variable, LSP doesn't track it

---

## Future-Proofing

### Design Principles for Long-term Maintainability

**1. Abstraction Layer**

```rust
// Don't expose tree-sitter or LSP directly
// Use unified abstractions

pub trait SymbolProvider {
    async fn get_symbols(&self, file: &str) -> Result<Vec<Symbol>, Error>;
    async fn search(&self, query: &str) -> Result<Vec<Symbol>, Error>;
}

// Tree-sitter implementation
impl SymbolProvider for TreeSitterIndex { ... }

// LSP implementation
impl SymbolProvider for LspManager { ... }

// Unified implementation (combines both)
impl SymbolProvider for UnifiedSymbolIndex { ... }
```

**Benefits:**
- Easy to add new providers (e.g., custom analyzers)
- Can swap implementations without breaking consumers
- Testable (mock providers)

---

**2. Feature Flags**

```rust
pub struct IntegrationConfig {
    pub use_tree_sitter: bool,
    pub use_lsp: bool,
    pub prefer_lsp_for_types: bool,
    pub enrich_symbols_async: bool,
    pub cache_enriched_symbols: bool,
}

impl Default for IntegrationConfig {
    fn default() -> Self {
        Self {
            use_tree_sitter: true,
            use_lsp: true,
            prefer_lsp_for_types: true,
            enrich_symbols_async: true,
            cache_enriched_symbols: true,
        }
    }
}
```

**Benefits:**
- Can disable LSP if it's slow/broken
- Can disable tree-sitter if LSP is sufficient
- A/B testing of different strategies

---

**3. Versioned APIs**

```rust
pub mod v1 {
    pub struct UnifiedSymbol { ... }
}

pub mod v2 {
    pub struct UnifiedSymbol {
        // New fields added
        pub v1_compat: v1::UnifiedSymbol,
        pub new_field: String,
    }
}

// Current version
pub use v2::*;
```

**Benefits:**
- Can evolve API without breaking existing code
- Gradual migration path
- Backward compatibility

---

**4. Metrics & Observability**

```rust
pub struct IntegrationMetrics {
    // Performance
    pub tree_sitter_parse_time: Histogram,
    pub lsp_request_time: Histogram,
    pub enrichment_time: Histogram,
    
    // Accuracy
    pub symbol_conflicts: Counter,
    pub lsp_unavailable: Counter,
    pub cache_hits: Counter,
    
    // Usage
    pub symbols_requested: Counter,
    pub context_assemblies: Counter,
    pub patches_applied: Counter,
}
```

**Benefits:**
- Identify performance bottlenecks
- Track conflict frequency
- Measure cache effectiveness
- Guide optimization efforts

---

## Success Criteria

### Performance Targets

| Operation | Target | Notes |
|-----------|--------|-------|
| Symbol search | <50ms | Tree-sitter fast path |
| Symbol enrichment | <200ms | LSP async |
| Context assembly | <300ms | Combined |
| Patch application | <500ms | Including LSP validation |

### Reliability Targets

| Metric | Target | Notes |
|--------|--------|-------|
| Symbol conflict rate | <5% | Tree-sitter vs LSP |
| Patch success rate | >80% | With LSP validation |
| LSP availability | >95% | Graceful degradation |
| Cache hit rate | >70% | For enriched symbols |

### User Experience Targets

| Metric | Target | Notes |
|--------|--------|-------|
| Time to first highlight | <10ms | Tree-sitter |
| Time to first diagnostic | <500ms | LSP |
| Completion latency | <100ms | LSP |
| Context token reduction | 40-60% | vs raw text |

---

## Conclusion

**Key Takeaways:**

1. **LSP and tree-sitter are complementary**
   - Tree-sitter: Fast syntax operations
   - LSP: Deep semantic analysis
   - Together: Best of both worlds

2. **Clear responsibility boundaries**
   - No duplication of effort
   - Each system does what it's best at
   - Unified API for consumers

3. **Graceful degradation**
   - Works without LSP (tree-sitter only)
   - Works without tree-sitter (LSP only)
   - Best experience with both

4. **Future-proof architecture**
   - Abstraction layers
   - Feature flags
   - Versioned APIs
   - Observable metrics

**Next Steps:**

1. Review and approve this RFC
2. Begin Phase 1 implementation (tree-sitter + LSP foundations)
3. Build integration layer (Week 3)
4. Test with real codebases
5. Iterate based on metrics

---

## References

- [RFC-003: Tree-sitter Integration](./RFC-003-tree-sitter-integration.md)
- [AI-LSP Integration](./AI_LSP_INTEGRATION.md)
- [ROADMAP: LSP Integration Strategy](./ROADMAP.md)
- [Language Server Protocol Specification](https://microsoft.github.io/language-server-protocol/)
- [Tree-sitter Documentation](https://tree-sitter.github.io/tree-sitter/)

---

**Document Version**: 1.0  
**Last Updated**: 2026-01-18  
**Status**: Draft - Awaiting Approval
