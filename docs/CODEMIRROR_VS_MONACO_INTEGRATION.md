# CodeMirror vs Monaco: Integration Challenges & Solutions

## Status

**Active** - Critical Architecture Document

## Created

2026-01-18

---

## The Challenge

**VSCode uses Monaco Editor:**
- Built-in LSP support (vscode-languageclient)
- Native tree-sitter integration (via extensions)
- Tight coupling with VS Code architecture
- Well-documented integration patterns

**ZaguanBlade uses CodeMirror 6:**
- No built-in LSP support
- No native tree-sitter integration
- Different extension architecture
- We must build everything from scratch

**This is both a challenge and an opportunity.**

---

## Why CodeMirror?

### Strategic Advantages

**1. Lightweight & Embeddable**
- Monaco: ~5MB minified
- CodeMirror: ~500KB minified
- **10x smaller bundle size**

**2. True Open Source**
- Monaco: MIT license but tightly coupled to VS Code
- CodeMirror: MIT license, truly independent
- **No Microsoft dependencies**

**3. Modular Architecture**
- Monaco: Monolithic, hard to customize deeply
- CodeMirror: Composable extensions, full control
- **Better for custom features**

**4. Performance**
- Monaco: Optimized for desktop
- CodeMirror: Optimized for web + desktop
- **Better Tauri integration**

**5. Lezer Parser**
- CodeMirror uses Lezer (similar to tree-sitter)
- Already has incremental parsing
- **Foundation for tree-sitter integration**

### Strategic Disadvantages

**1. No Built-in LSP**
- Must implement LSP client ourselves
- Must handle all LSP protocol details
- Must integrate with CodeMirror's extension system

**2. Smaller Ecosystem**
- Fewer examples and libraries
- Less community knowledge
- More pioneering work required

**3. Integration Complexity**
- Tree-sitter + CodeMirror = custom integration
- LSP + CodeMirror = custom integration
- AI + CodeMirror = custom integration

---

## Integration Architecture

### The ZaguanBlade Approach

```
┌─────────────────────────────────────────────────────────────┐
│                    CodeMirror 6 Core                         │
│  - Document model                                            │
│  - View layer                                                │
│  - Extension system                                          │
└─────────────────────────────────────────────────────────────┘
                          ↕
┌─────────────────────────────────────────────────────────────┐
│              Custom Integration Layer (Our Code)             │
│                                                              │
│  ┌─────────────────┬──────────────────┬──────────────────┐ │
│  │ Tree-sitter     │ LSP Client       │ AI Integration   │ │
│  │ Extension       │ Extension        │ Extension        │ │
│  │                 │                  │                  │ │
│  │ • StateField    │ • StateField     │ • StateField     │ │
│  │ • ViewPlugin    │ • ViewPlugin     │ • ViewPlugin     │ │
│  │ • Decorations   │ • Decorations    │ • Decorations    │ │
│  └─────────────────┴──────────────────┴──────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                          ↕
┌─────────────────────────────────────────────────────────────┐
│                    Tauri Backend (Rust)                      │
│  - Tree-sitter native                                        │
│  - LSP Manager                                               │
│  - AI Workflow                                               │
└─────────────────────────────────────────────────────────────┘
```

---

## Deep Integration Challenges

### Challenge 1: LSP Protocol in CodeMirror

**Monaco has:**
```typescript
import * as monaco from 'monaco-editor';
import { MonacoLanguageClient } from 'monaco-languageclient';

// Built-in, works out of the box
const client = new MonacoLanguageClient({
  name: 'TypeScript',
  clientOptions: { ... }
});
```

**CodeMirror needs:**
```typescript
// We must build this ourselves
import { StateField, StateEffect } from '@codemirror/state';
import { ViewPlugin, Decoration } from '@codemirror/view';

// Custom LSP integration
const lspDiagnostics = StateField.define<Diagnostic[]>({
  create() { return []; },
  update(diagnostics, tr) {
    // Handle LSP diagnostic updates
    for (let effect of tr.effects) {
      if (effect.is(addDiagnostic)) {
        diagnostics = [...diagnostics, effect.value];
      }
    }
    return diagnostics;
  }
});

const diagnosticGutter = ViewPlugin.fromClass(class {
  constructor(view: EditorView) {
    // Render diagnostics in gutter
  }
  
  update(update: ViewUpdate) {
    // Update on document changes
  }
});
```

**Our Solution:**
```typescript
// src/components/editor/extensions/lsp/index.ts

export function createLspExtension(config: LspConfig) {
  return [
    // State management
    lspDiagnostics,
    lspCompletions,
    lspHover,
    
    // UI rendering
    diagnosticGutter,
    diagnosticDecorations,
    hoverTooltip,
    completionSource,
    
    // Event handlers
    lspEventHandler,
  ];
}
```

### Challenge 2: Tree-sitter in CodeMirror

**Monaco has:**
```typescript
// Tree-sitter via extensions (community)
import { TreeSitterLanguage } from 'monaco-tree-sitter';

monaco.languages.register({ id: 'typescript' });
monaco.languages.setMonarchTokensProvider('typescript', TreeSitterLanguage);
```

**CodeMirror + Lezer:**
```typescript
// CodeMirror already uses Lezer (similar to tree-sitter)
import { parser } from '@lezer/javascript';
import { LRLanguage } from '@codemirror/language';

const jsLanguage = LRLanguage.define({ parser });
```

**Our Challenge:**
- Lezer is good but not as powerful as tree-sitter
- We want tree-sitter for semantic features
- Must integrate tree-sitter alongside Lezer

**Our Solution:**
```typescript
// Dual-parser approach

// 1. Lezer for syntax highlighting (fast, built-in)
import { javascript } from '@codemirror/lang-javascript';

// 2. Tree-sitter for semantic analysis (powerful, custom)
import Parser from 'web-tree-sitter';

const treeSitterField = StateField.define<Tree | null>({
  create(state) {
    // Parse with tree-sitter in background
    parseInBackground(state.doc.toString());
    return null;
  },
  
  update(tree, tr) {
    if (!tr.docChanged) return tree;
    
    // Incremental parse
    if (tree) {
      tree.edit({
        startIndex: tr.changes.from,
        oldEndIndex: tr.changes.from + tr.changes.length,
        newEndIndex: tr.changes.from + tr.changes.insert.length,
        // ... position info
      });
    }
    
    // Re-parse in background
    parseInBackground(tr.state.doc.toString(), tree);
    return tree;
  }
});

// Use tree-sitter tree for:
// - Symbol extraction
// - Semantic folding
// - Structural search
// - AI context assembly

// Use Lezer for:
// - Syntax highlighting (fast)
// - Basic folding (fast)
```

### Challenge 3: AI Integration in CodeMirror

**Monaco + Copilot:**
```typescript
// GitHub Copilot extension for VS Code
// Built-in inline suggestions
// Ghost text rendering
// Automatic integration
```

**CodeMirror needs:**
```typescript
// Custom AI suggestion rendering

const aiSuggestions = StateField.define<AiSuggestion[]>({
  create() { return []; },
  update(suggestions, tr) {
    // Handle AI suggestion updates from backend
  }
});

const aiSuggestionWidget = ViewPlugin.fromClass(class {
  decorations: DecorationSet;
  
  constructor(view: EditorView) {
    this.decorations = this.buildDecorations(view);
  }
  
  update(update: ViewUpdate) {
    // Render AI suggestions as ghost text
    const suggestions = update.state.field(aiSuggestions);
    this.decorations = this.buildDecorations(update.view);
  }
  
  buildDecorations(view: EditorView) {
    const suggestions = view.state.field(aiSuggestions);
    const decorations = [];
    
    for (const suggestion of suggestions) {
      decorations.push(
        Decoration.widget({
          widget: new AiSuggestionWidget(suggestion),
          side: 1,
        }).range(suggestion.position)
      );
    }
    
    return Decoration.set(decorations);
  }
});
```

---

## Our Integration Strategy

### 1. Extension Architecture

**CodeMirror's Strength: Composable Extensions**

```typescript
// src/components/editor/extensions/index.ts

export function createZaguanExtensions(config: ZaguanConfig) {
  return [
    // Core CodeMirror
    basicSetup,
    
    // Language support (Lezer)
    javascript({ jsx: true, typescript: true }),
    
    // Tree-sitter integration (our custom)
    createTreeSitterExtension({
      language: 'typescript',
      parser: await loadTreeSitterParser('typescript'),
    }),
    
    // LSP integration (our custom)
    createLspExtension({
      serverUrl: config.lspServerUrl,
      capabilities: ['diagnostics', 'completion', 'hover'],
    }),
    
    // AI integration (our custom)
    createAiExtension({
      backend: config.aiBackend,
      features: ['suggestions', 'fixes', 'refactorings'],
    }),
    
    // Zaguan-specific features
    verticalDiffBlocks,
    semanticPatches,
    contextHighlighting,
  ];
}
```

### 2. State Coordination

**Challenge:** Multiple systems updating the same document

**Solution:** Centralized state management

```typescript
// src/components/editor/state/coordinator.ts

export class StateCoordinator {
  private treeSitterTree: Tree | null = null;
  private lspDiagnostics: Diagnostic[] = [];
  private aiSuggestions: AiSuggestion[] = [];
  
  // Coordinate updates
  async onDocumentChange(change: ChangeSet) {
    // 1. Update tree-sitter (fast, incremental)
    this.treeSitterTree = await this.updateTreeSitter(change);
    
    // 2. Notify LSP (debounced)
    this.debouncedLspUpdate(change);
    
    // 3. Update AI context (if needed)
    if (this.shouldUpdateAiContext(change)) {
      this.updateAiContext(this.treeSitterTree);
    }
  }
  
  // Resolve conflicts
  resolveDiagnosticConflict(
    treeSitterError: SyntaxError,
    lspDiagnostic: Diagnostic
  ): Diagnostic {
    // Prefer LSP for semantic errors
    // Prefer tree-sitter for syntax errors
    return lspDiagnostic.severity === 'error' 
      ? lspDiagnostic 
      : treeSitterError;
  }
}
```

### 3. Performance Optimization

**Challenge:** Multiple parsers/analyzers can be slow

**Solution:** Smart scheduling and caching

```typescript
// src/components/editor/performance/scheduler.ts

export class OperationScheduler {
  private queue: PriorityQueue<Operation> = new PriorityQueue();
  
  schedule(operation: Operation) {
    // Priority levels:
    // 1. User input (highest) - must be instant
    // 2. Syntax highlighting - very fast
    // 3. Tree-sitter parsing - fast
    // 4. LSP requests - medium
    // 5. AI operations - low
    
    this.queue.enqueue(operation, operation.priority);
    this.processQueue();
  }
  
  async processQueue() {
    while (!this.queue.isEmpty()) {
      const op = this.queue.dequeue();
      
      // Yield to browser between operations
      await this.yieldToMain();
      
      // Execute operation
      await op.execute();
    }
  }
  
  private async yieldToMain() {
    return new Promise(resolve => setTimeout(resolve, 0));
  }
}
```

---

## Advantages of Our Approach

### 1. Full Control

**Monaco/VS Code:**
- Constrained by VS Code's architecture
- Must work within their extension API
- Limited customization depth

**CodeMirror/ZaguanBlade:**
- ✅ Complete control over integration
- ✅ Can optimize for our specific use case
- ✅ No artificial limitations

### 2. Lightweight

**Monaco bundle:**
- ~5MB minified
- Heavy on resources
- Slower startup

**CodeMirror bundle:**
- ✅ ~500KB base + our extensions (~1MB total)
- ✅ Faster startup
- ✅ Better for Tauri (smaller app size)

### 3. Custom Features

**What we can do that VS Code can't:**

```typescript
// Vertical Diff Blocks (our innovation)
const verticalDiffBlocks = ViewPlugin.fromClass(class {
  // Show AI changes as vertical blocks, not inline
  // Better UX for large changes
});

// Semantic Patches (our innovation)
const semanticPatches = StateField.define({
  // Apply patches at AST level, not text level
  // More reliable than text-based diffs
});

// Context-Aware Highlighting (our innovation)
const contextHighlighting = ViewPlugin.fromClass(class {
  // Highlight code based on AI context
  // Show what the AI is "looking at"
});
```

### 4. Tauri Optimization

**Monaco in Tauri:**
- Designed for Electron
- Heavy IPC overhead
- Not optimized for Tauri

**CodeMirror in Tauri:**
- ✅ Lightweight, less IPC
- ✅ Can use Rust backend for heavy operations
- ✅ Better performance characteristics

---

## Implementation Roadmap

### Phase 1: Foundation (Weeks 1-2)

**Goal:** Basic LSP + Tree-sitter working

```typescript
// Week 1: LSP Extension
- [ ] Implement LSP client in TypeScript
- [ ] Create CodeMirror StateField for diagnostics
- [ ] Render diagnostics in gutter
- [ ] Test with TypeScript language server

// Week 2: Tree-sitter Extension
- [ ] Load web-tree-sitter WASM
- [ ] Create StateField for parse tree
- [ ] Implement incremental parsing
- [ ] Test with TypeScript grammar
```

### Phase 2: Integration (Weeks 3-4)

**Goal:** LSP + Tree-sitter working together

```typescript
// Week 3: State Coordination
- [ ] Build StateCoordinator
- [ ] Implement conflict resolution
- [ ] Add caching layer
- [ ] Performance testing

// Week 4: UI Integration
- [ ] Unified diagnostic display
- [ ] Hover tooltips (LSP + tree-sitter)
- [ ] Completion UI
- [ ] Symbol outline
```

### Phase 3: AI Features (Weeks 5-6)

**Goal:** AI integrated with LSP + Tree-sitter

```typescript
// Week 5: AI Extension
- [ ] AI suggestion rendering
- [ ] Semantic patch application
- [ ] Context highlighting
- [ ] Vertical diff blocks

// Week 6: Self-Correction Loop
- [ ] LSP errors → AI feedback
- [ ] AI self-correction
- [ ] Validation pipeline
- [ ] Success metrics
```

---

## Comparison: Monaco vs CodeMirror Integration

| Feature | Monaco (VS Code) | CodeMirror (ZaguanBlade) |
|---------|------------------|--------------------------|
| **LSP Support** | Built-in | Custom (more work, more control) |
| **Tree-sitter** | Via extensions | Custom (full integration) |
| **Bundle Size** | ~5MB | ~1MB |
| **Customization** | Limited | Unlimited |
| **Performance** | Good | Excellent (optimized for our use case) |
| **AI Integration** | Via extensions | Native (deep integration) |
| **Vertical Diffs** | ❌ Not possible | ✅ Our innovation |
| **Semantic Patches** | ❌ Text-based only | ✅ AST-based |
| **Tauri Optimization** | ❌ Electron-focused | ✅ Tauri-optimized |
| **Development Effort** | Low (use existing) | High (build custom) |
| **Long-term Flexibility** | Low (constrained) | High (full control) |

---

## Key Insights

### 1. More Work, More Reward

**Short-term:**
- ❌ More implementation work
- ❌ More debugging
- ❌ More documentation needed

**Long-term:**
- ✅ Complete control
- ✅ Unique features
- ✅ Better performance
- ✅ Competitive advantage

### 2. CodeMirror's Extension System is Powerful

**Monaco extensions:**
- Limited to what VS Code allows
- Must work within constraints

**CodeMirror extensions:**
- ✅ Can do anything
- ✅ Full access to internals
- ✅ Composable architecture

### 3. Our Integration is a Moat

**Competitors using Monaco:**
- Easy to start
- Hard to differentiate
- Constrained by VS Code

**ZaguanBlade using CodeMirror:**
- ✅ Harder to start
- ✅ Easy to differentiate
- ✅ Unlimited potential
- ✅ **Competitive moat**

---

## Success Criteria

### Performance Targets

| Operation | Target | Notes |
|-----------|--------|-------|
| Syntax highlighting | <5ms | Lezer (built-in) |
| Tree-sitter parse | <20ms | Incremental |
| LSP diagnostic | <200ms | Async |
| AI suggestion | <500ms | Background |
| Document open | <100ms | Total time to interactive |

### Feature Parity with VS Code

- ✅ Diagnostics (LSP)
- ✅ Completions (LSP)
- ✅ Hover info (LSP)
- ✅ Go to definition (LSP)
- ✅ Find references (LSP)
- ✅ Rename (LSP)
- ✅ Code actions (LSP)
- ✅ Formatting (LSP)

### Beyond VS Code

- ✅ Vertical diff blocks (our innovation)
- ✅ Semantic patches (our innovation)
- ✅ AI self-correction (our innovation)
- ✅ Context-aware highlighting (our innovation)
- ✅ Tree-sitter + LSP fusion (our innovation)

---

## Conclusion

**The Challenge:**
CodeMirror requires more integration work than Monaco.

**The Opportunity:**
This extra work gives us complete control and enables unique features.

**The Strategy:**
Build deep, custom integrations that create a competitive moat.

**The Result:**
A more powerful, more flexible, more innovative editor than VS Code.

**Key Principle:**
The difficulty of integration is not a bug, it's a feature. It's what will make ZaguanBlade unique and defensible.

---

## Next Steps

1. **Review this document** - Ensure strategy aligns with vision
2. **Prioritize integrations** - LSP first? Tree-sitter first? Both parallel?
3. **Prototype key features** - Validate approach with working code
4. **Document patterns** - Create reusable integration patterns
5. **Build incrementally** - One extension at a time, test thoroughly

---

**Document Version**: 1.0  
**Last Updated**: 2026-01-18  
**Status**: Active - Critical Architecture Document
