# Language Intelligence Implementation Plan

## Status: IN PROGRESS (Phase 1 Complete, Phase 2 Active)

**Last Updated:** 2026-01-19  
**Goal:** Make Zaguán Blade's language intelligence **top of class** — matching or exceeding Cursor/Windsurf/VS Code.

---

## Executive Summary

Zaguán Blade uses a **hybrid architecture** for language intelligence:

| Layer | Technology | Purpose | Status |
|-------|------------|---------|--------|
| **Visual (Frontend)** | CodeMirror + Lezer | Syntax highlighting, basic editing | ✅ Complete |
| **Semantic (Backend)** | Tree-sitter + SQLite | Symbol extraction, persistent index, AI context | ✅ Core Complete |
| **Intelligence (Backend)** | LSP Clients | Hover, Completions, Diagnostics, Go-to-Def | 🔄 Partial |
| **AI Context** | LanguageService + SymbolStore | Code understanding for AI prompts | ✅ Core Complete |

---

## Current State Assessment

### ✅ What's Working

| Feature | Location | Notes |
|---------|----------|-------|
| Tree-sitter parsing | `src-tauri/src/tree_sitter/` | TypeScript, JavaScript, Python, Rust |
| Symbol extraction | `tree_sitter/symbol.rs` | Functions, classes, methods, variables |
| SQLite symbol store | `src-tauri/src/symbol_index/` | Persistent storage with FTS5 search |
| LanguageService facade | `src-tauri/src/language_service/` | Unified API for all language ops |
| LSP Manager | `src-tauri/src/lsp/` | Spawns external servers (ts-server, rust-analyzer) |
| BCP Language Domain | `blade_protocol.rs` | Intents/Events for Language operations |
| Frontend completions | `languageFeatures.ts` | CodeMirror autocomplete via backend |
| Go-to-Definition (F12) | `languageFeatures.ts` + `EditorPanel.tsx` | Cross-file navigation |
| Settings toggle | `SettingsModal.tsx` | Enable/disable LSP |
| didOpen/didChange | `CodeEditor.tsx` + `handler.rs` | LSP document sync on file open/edit |
| Diagnostics display | `diagnostics.ts` | Red/yellow/blue underlines for errors |
| Auto-index on open | `lib.rs` (open_workspace) | Background indexing when workspace opens |
| Signature Help | `signatureHelp.ts` | Parameter hints on '(' and ',' |

### 🔄 What's Partially Working

| Feature | Issue | Priority |
|---------|-------|----------|
| **Hover tooltips** | `GetHover` returns data, tooltip CSS needs polish | MEDIUM |
| **Code Actions** | Backend complete, needs frontend UI (lightbulb menu) | MEDIUM |

### ❌ What's Missing

| Feature | Description | Priority |
|---------|-------------|----------|
| **Find references** | Where is this symbol used? (backend ready, UI missing) | MEDIUM |
| **Rename symbol** | Refactor across files | MEDIUM |
| **Document symbols** | Outline view / breadcrumbs | LOW |
| **Auto-import** | Add missing imports automatically | LOW |
| **Inlay hints** | Type annotations inline | LOW |

---

## Phase 1: Foundation ✅ COMPLETE

### Week 1-2: Tree-sitter Backend
- [x] Tree-sitter dependency and parser setup
- [x] Symbol extraction for TS/JS/Python/Rust
- [x] Query patterns for functions, classes, methods
- [x] SymbolStore with SQLite + FTS5

### Week 3: LSP Manager
- [x] LSP client with JSON-RPC communication
- [x] Server lifecycle management
- [x] Basic operations (hover, completions, definition)
- [x] Pre-configured servers (typescript-language-server, rust-analyzer, pylsp)

### Week 4: BCP Integration
- [x] Language domain intents/events  
- [x] LanguageHandler dispatch
- [x] Frontend LanguageService wrapper
- [x] Basic completions in CodeMirror

---

## Phase 2: Core IDE Features 🔄 IN PROGRESS

### 2.1 Fix Hover Tooltips [HIGH PRIORITY]

**Problem:** `GetHover` returns data but CodeMirror doesn't show it.

**Files to modify:**
- `src/components/editor/extensions/languageFeatures.ts`

**Implementation:**

```typescript
// In languageFeatures.ts - Add hover tooltip extension

import { hoverTooltip, Tooltip } from '@codemirror/view';
import { LanguageService } from '@/services/LanguageService';

function createHoverExtension(filename: string): Extension {
  return hoverTooltip(async (view, pos): Promise<Tooltip | null> => {
    const line = view.state.doc.lineAt(pos);
    const lineNumber = line.number;
    const column = pos - line.from;
    
    try {
      const hover = await LanguageService.getHover(filename, lineNumber, column);
      if (!hover || !hover.contents) return null;
      
      // Extract markdown content from hover response
      const content = typeof hover.contents === 'string' 
        ? hover.contents 
        : hover.contents.value || '';
      
      if (!content.trim()) return null;
      
      return {
        pos,
        above: true,
        create() {
          const dom = document.createElement('div');
          dom.className = 'cm-hover-tooltip';
          dom.innerHTML = renderMarkdown(content); // Use marked or similar
          return { dom };
        }
      };
    } catch (e) {
      console.error('[Hover] Failed:', e);
      return null;
    }
  }, { hoverTime: 300 });
}
```

**CSS needed:**
```css
/* Add to index.css */
.cm-hover-tooltip {
  background: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  border-radius: 6px;
  padding: 8px 12px;
  max-width: 500px;
  max-height: 300px;
  overflow: auto;
  font-size: 13px;
  box-shadow: 0 4px 12px rgba(0,0,0,0.3);
}

.cm-hover-tooltip code {
  background: var(--bg-app);
  padding: 2px 4px;
  border-radius: 3px;
  font-family: var(--font-mono);
}
```

---

### 2.2 Implement Diagnostics Display [HIGH PRIORITY]

**Problem:** LSP sends `textDocument/publishDiagnostics` but we don't render them.

**Architecture:**

```
LSP Server → JSON-RPC → LspManager → DiagnosticsUpdated event → Frontend
                                                                    ↓
                                              CodeMirror ← setDiagnostics decoration
```

**Backend (Already exists, verify working):**
```rust
// In lsp/manager.rs - Handle notifications
if method == "textDocument/publishDiagnostics" {
    let params: PublishDiagnosticsParams = serde_json::from_value(params)?;
    // Emit to frontend
    self.emit_diagnostics(params.uri, params.diagnostics);
}
```

**Frontend Implementation:**

```typescript
// src/components/editor/extensions/diagnostics.ts

import { Decoration, DecorationSet, EditorView, ViewPlugin, ViewUpdate } from '@codemirror/view';
import { StateField, StateEffect } from '@codemirror/state';
import { listen } from '@tauri-apps/api/event';

interface Diagnostic {
  range: { start: { line: number; character: number }; end: { line: number; character: number } };
  severity: 1 | 2 | 3 | 4; // Error, Warning, Info, Hint
  message: string;
  source?: string;
}

// Effect to update diagnostics
const setDiagnostics = StateEffect.define<Diagnostic[]>();

// State field to store current diagnostics
const diagnosticsState = StateField.define<Diagnostic[]>({
  create: () => [],
  update(diagnostics, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setDiagnostics)) {
        return effect.value;
      }
    }
    return diagnostics;
  }
});

// Decorations for underlines
const diagnosticDecorations = EditorView.decorations.compute(
  [diagnosticsState],
  (state) => {
    const diagnostics = state.field(diagnosticsState);
    const decorations: Range<Decoration>[] = [];
    
    for (const diag of diagnostics) {
      try {
        const startLine = state.doc.line(diag.range.start.line + 1);
        const endLine = state.doc.line(diag.range.end.line + 1);
        const from = startLine.from + diag.range.start.character;
        const to = endLine.from + diag.range.end.character;
        
        const className = diag.severity === 1 ? 'cm-diagnostic-error' 
                        : diag.severity === 2 ? 'cm-diagnostic-warning'
                        : 'cm-diagnostic-info';
        
        decorations.push(Decoration.mark({ class: className }).range(from, to));
      } catch {}
    }
    
    return Decoration.set(decorations.sort((a, b) => a.from - b.from));
  }
);

export function diagnosticsExtension(filename: string): Extension {
  return [
    diagnosticsState,
    diagnosticDecorations,
    EditorView.domEventHandlers({
      // Initialize listener on mount
    }),
    // Plugin to listen for diagnostic events
    ViewPlugin.define(view => {
      const unlisten = listen('blade-event', (event) => {
        const payload = event.payload as any;
        if (payload.event?.type === 'Language' && 
            payload.event.payload?.type === 'DiagnosticsUpdated' &&
            payload.event.payload.file_path === filename) {
          view.dispatch({
            effects: setDiagnostics.of(payload.event.payload.diagnostics)
          });
        }
      });
      
      return {
        destroy() {
          unlisten.then(fn => fn());
        }
      };
    })
  ];
}
```

**CSS:**
```css
.cm-diagnostic-error {
  text-decoration: underline wavy #ef4444;
  text-decoration-skip-ink: none;
}

.cm-diagnostic-warning {
  text-decoration: underline wavy #f59e0b;
  text-decoration-skip-ink: none;
}

.cm-diagnostic-info {
  text-decoration: underline wavy #3b82f6;
  text-decoration-skip-ink: none;
}
```

---

### 2.3 Implement didOpen/didChange Notifications [MEDIUM PRIORITY]

**Problem:** LSP servers don't know about unsaved changes, so they analyze stale content.

**Solution:** Send `textDocument/didOpen` when file opens, `textDocument/didChange` on edits.

**Frontend → Backend flow:**

```typescript
// When file opens in editor
await LanguageService.didOpen(filename, content, languageId);

// On each edit (debounced)
const debouncedChange = debounce(async (content: string) => {
  await LanguageService.didChange(filename, content);
}, 100);
```

**Backend implementation:**
```rust
// In language_service/service.rs

pub async fn did_open(&self, file_path: &str, content: &str, language_id: &str) -> Result<(), Error> {
    if let Some(ref lsp) = *self.lsp_manager.read().unwrap() {
        lsp.send_notification("textDocument/didOpen", json!({
            "textDocument": {
                "uri": format!("file://{}", file_path),
                "languageId": language_id,
                "version": 1,
                "text": content
            }
        })).await?;
    }
    Ok(())
}

pub async fn did_change(&self, file_path: &str, content: &str) -> Result<(), Error> {
    // Increment version and send full content
    // (Full sync is simpler; incremental sync is optimization)
}
```

---

### 2.4 Auto-Index on Workspace Open [MEDIUM PRIORITY]

**Problem:** Symbol index is empty until manually triggered.

**Solution:** Index workspace on open, watch for file changes.

```rust
// In AppState::new() or open_workspace handler

// On workspace open
if settings.editor.enable_lsp {
    tokio::spawn(async move {
        if let Err(e) = language_service.index_workspace().await {
            eprintln!("[LanguageService] Workspace indexing failed: {}", e);
        }
    });
}

// File watcher integration (already exists in lib.rs)
// Add: on file change → language_service.index_file(path)
```

---

## Phase 3: Advanced IDE Features

### 3.1 Find All References

```typescript
// Frontend
const references = await LanguageService.getReferences(filename, line, col);
// Display in panel or peek view
```

```rust
// Backend - Already implemented in service.rs
pub fn get_references(&self, file_path: &str, line: u32, col: u32) -> Result<Vec<Location>, Error>
```

### 3.2 Rename Symbol

```rust
// Backend
pub async fn rename_symbol(
    &self, 
    file_path: &str, 
    line: u32, 
    col: u32, 
    new_name: &str
) -> Result<WorkspaceEdit, Error> {
    // 1. Get LSP workspace edit
    // 2. Convert to multi-file changes
    // 3. Return for preview/apply
}
```

### 3.3 Quick Fixes / Code Actions

```rust
// When user clicks on diagnostic or uses Ctrl+.
pub async fn get_code_actions(
    &self,
    file_path: &str,
    range: Range,
    diagnostics: Vec<Diagnostic>
) -> Result<Vec<CodeAction>, Error>
```

---

## Phase 4: AI Integration

### 4.1 Context Assembly for AI Prompts

**Already implemented in `context_assembly/assembler.rs`:**
- Cursor-based context (current file + nearby symbols)
- Query-based context (search symbols matching task)
- Budget management (token limits)

**To add:**
- Include diagnostics in context ("These are the current errors...")
- Include related files (imports, dependencies)
- Include recent edits context

### 4.2 AI-Powered Diagnostics Fixing

```rust
// When AI generates code, validate with LSP
pub async fn validate_and_fix(&self, file_path: &str, content: &str) -> Result<String, Error> {
    // 1. Write content to virtual buffer
    // 2. Get diagnostics from LSP
    // 3. If errors, ask AI to fix with diagnostic context
    // 4. Repeat until clean or max iterations
}
```

---

## Implementation Priority Queue

### Sprint 1 (This Week)
1. [ ] **Fix Hover Tooltips** - Show hover data in editor
2. [ ] **Implement Diagnostics** - Red squiggles for errors
3. [ ] **Wire didOpen/didChange** - Fresh LSP results

### Sprint 2 (Next Week)
4. [ ] **Auto-index workspace** - Index on open
5. [ ] **Find References UI** - Panel or peek view
6. [ ] **Signature Help** - Parameter hints

### Sprint 3 (Following Week)
7. [ ] **Code Actions** - Quick fixes
8. [ ] **Rename Symbol** - Cross-file refactoring
9. [ ] **Document Outline** - Symbol tree view

### Sprint 4 (Future)
10. [ ] **Inlay Hints** - Type annotations
11. [ ] **Auto-Import** - Add missing imports
12. [ ] **AI Validation** - LSP-validated AI edits

---

## File Structure Reference

```
src-tauri/src/
├── tree_sitter/
│   ├── mod.rs          # Public exports
│   ├── parser.rs       # Multi-language parser
│   ├── symbol.rs       # Symbol extraction
│   └── query.rs        # Tree-sitter queries
├── lsp/
│   ├── mod.rs          # Public exports
│   ├── client.rs       # JSON-RPC client
│   ├── manager.rs      # Server lifecycle
│   └── types.rs        # LSP type conversions
├── language_service/
│   ├── mod.rs          # Public exports
│   ├── service.rs      # Unified facade
│   ├── indexer.rs      # File discovery
│   └── handler.rs      # BCP intent handler
├── symbol_index/
│   ├── mod.rs          # Public exports
│   ├── store.rs        # SQLite operations
│   └── search.rs       # FTS5 search
└── context_assembly/
    ├── mod.rs          # Public exports
    ├── assembler.rs    # Context builder
    ├── budget.rs       # Token management
    └── strategy.rs     # Selection strategies

src/
├── services/
│   └── LanguageService.ts   # Frontend wrapper
├── components/
│   └── editor/
│       └── extensions/
│           ├── languageFeatures.ts  # Completions, Go-to-Def
│           ├── diagnostics.ts       # Error underlines [TODO]
│           └── hover.ts             # Hover tooltips [TODO]
```

---

## Performance Targets

| Operation | Target | Current |
|-----------|--------|---------|
| Parse 1000 lines | <20ms | ✅ ~10ms |
| Symbol search | <50ms | ✅ ~30ms |
| Completions | <100ms | 🔄 ~150ms |
| Hover | <50ms | ✅ ~40ms |
| Diagnostics update | <200ms | 🔄 Not measured |
| Workspace index (1000 files) | <10s | 🔄 Not measured |

---

## Success Criteria

Zaguán Blade is "top of class" when:

1. **Instant Feedback**: Errors appear as you type (like VS Code)
2. **Smart Completions**: Context-aware suggestions from LSP
3. **Reliable Navigation**: Go-to-definition works cross-file
4. **Hover Intelligence**: Types and docs on mouse hover
5. **AI Context**: Symbol-aware AI prompts that "see" your code structure
6. **Fast Indexing**: Background indexing doesn't block UI
7. **Zero External Setup**: Works without user installing language servers (stretch goal)

---

## Notes

- **External LSPs**: Currently requires `typescript-language-server`, `rust-analyzer`, `pylsp` installed. Future: Consider bundling or auto-downloading.
- **Memory**: Symbol index stays in SQLite (disk), not RAM. Tree-sitter ASTs are ephemeral.
- **Incremental**: Tree-sitter supports incremental parsing. Use `old_tree` when available.
