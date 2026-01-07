# zblade Development Roadmap

This document outlines the strategic vision and implementation plans for zblade's core features.

---

## Core Feature Pillars

### 1. LSP Integration (Language Server Protocol)
### 2. Tree-sitter Integration  
### 3. Theming System
### 4. AI-LSP Integration (The "Magic" Feature)

---

## 1. LSP Integration Strategy

### Overview

LSP (Language Server Protocol) is **essential** for a modern IDE. It provides:
- Code completion (IntelliSense)
- Go to definition/references
- Hover information (type hints, documentation)
- Diagnostics (errors, warnings)
- Code actions (quick fixes, refactorings)
- Rename symbol
- Document formatting

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    zblade Frontend                       │
│  ┌────────────────────────────────────────────────────┐ │
│  │  CodeMirror Editor                                 │ │
│  │  - Displays diagnostics                            │ │
│  │  - Shows completions                               │ │
│  │  - Renders hover info                              │ │
│  └────────────────────────────────────────────────────┘ │
│                          ↕ (events)                      │
│  ┌────────────────────────────────────────────────────┐ │
│  │  LSP Client (TypeScript)                           │ │
│  │  - Manages LSP requests                            │ │
│  │  - Caches responses                                │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
                          ↕ (Tauri commands)
┌─────────────────────────────────────────────────────────┐
│                    zblade Backend (Rust)                 │
│  ┌────────────────────────────────────────────────────┐ │
│  │  LSP Manager                                       │ │
│  │  - Spawns language servers                         │ │
│  │  - Routes requests to correct server               │ │
│  │  - Manages server lifecycle                        │ │
│  └────────────────────────────────────────────────────┘ │
│                          ↕ (JSON-RPC)                    │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Language Servers (rust-analyzer, gopls, etc.)     │ │
│  │  - One per language                                │ │
│  │  - Spawned as child processes                      │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### Implementation Plan

#### Phase 1: Foundation (Week 1-2)

**Backend (Rust):**
```rust
// src-tauri/src/lsp/mod.rs
pub struct LspManager {
    servers: HashMap<String, LspServer>,  // language -> server
}

pub struct LspServer {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    capabilities: ServerCapabilities,
}

impl LspManager {
    pub fn spawn_server(&mut self, language: &str) -> Result<(), String> {
        // Spawn rust-analyzer, gopls, typescript-language-server, etc.
    }
    
    pub async fn send_request(&mut self, 
        language: &str, 
        method: &str, 
        params: Value
    ) -> Result<Value, String> {
        // JSON-RPC request/response
    }
}
```

**Tauri Commands:**
```rust
#[tauri::command]
async fn lsp_initialize(language: String, workspace_root: String) -> Result<(), String>

#[tauri::command]
async fn lsp_completion(
    language: String, 
    file_path: String, 
    line: usize, 
    column: usize
) -> Result<Vec<CompletionItem>, String>

#[tauri::command]
async fn lsp_hover(
    language: String,
    file_path: String,
    line: usize,
    column: usize
) -> Result<Option<HoverInfo>, String>

#[tauri::command]
async fn lsp_goto_definition(
    language: String,
    file_path: String,
    line: usize,
    column: usize
) -> Result<Vec<Location>, String>
```

**Events:**
```rust
// Diagnostics pushed from server
"lsp-diagnostics-updated" -> DiagnosticsPayload
"lsp-server-started" -> LspServerStartedPayload
"lsp-server-error" -> LspServerErrorPayload
```

#### Phase 2: Core Features (Week 3-4)

**Features to implement:**
1. ✅ Initialize LSP server on workspace open
2. ✅ Code completion (textDocument/completion)
3. ✅ Diagnostics (textDocument/publishDiagnostics)
4. ✅ Hover info (textDocument/hover)
5. ✅ Go to definition (textDocument/definition)

**Frontend Integration:**
```typescript
// src/hooks/useLsp.ts
export function useLsp(language: string) {
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
  
  useEffect(() => {
    // Listen for diagnostics
    const unlisten = listen<DiagnosticsPayload>('lsp-diagnostics-updated', (event) => {
      if (event.payload.language === language) {
        setDiagnostics(event.payload.diagnostics);
      }
    });
    return () => unlisten.then(fn => fn());
  }, [language]);
  
  const getCompletions = async (filePath: string, line: number, column: number) => {
    return await invoke<CompletionItem[]>('lsp_completion', {
      language,
      filePath,
      line,
      column
    });
  };
  
  return { diagnostics, getCompletions };
}
```

#### Phase 3: Advanced Features (Week 5-6)

**Features:**
6. ✅ Find references (textDocument/references)
7. ✅ Rename symbol (textDocument/rename)
8. ✅ Code actions (textDocument/codeAction)
9. ✅ Document formatting (textDocument/formatting)
10. ✅ Signature help (textDocument/signatureHelp)

#### Phase 4: Polish (Week 7-8)

**Features:**
11. ✅ Incremental document sync (for performance)
12. ✅ Workspace symbols (workspace/symbol)
13. ✅ Document symbols (textDocument/documentSymbol)
14. ✅ Server restart on crash
15. ✅ Configuration per language

### Language Server Binaries

**Bundling Strategy:**

Option A: **Bundle with app** (Recommended)
- Include language servers in app bundle
- Guaranteed versions, no user setup
- Larger app size (~50-100MB per server)

Option B: **Download on demand**
- Download servers when first needed
- Smaller initial download
- Requires internet connection

**Priority Languages:**
1. **Rust** - rust-analyzer (primary use case)
2. **Go** - gopls
3. **TypeScript/JavaScript** - typescript-language-server
4. **Python** - pyright or pylsp
5. **C/C++** - clangd

### Configuration

**Per-language settings:**
```json
// ~/.zblade/lsp-config.json
{
  "rust": {
    "server": "rust-analyzer",
    "args": [],
    "initializationOptions": {
      "cargo": {
        "allFeatures": true
      }
    }
  },
  "go": {
    "server": "gopls",
    "args": ["-mode=stdio"],
    "initializationOptions": {}
  }
}
```

### Performance Considerations

1. **Debounce requests** - Don't spam completion on every keystroke
2. **Cache responses** - Store hover info, definitions
3. **Incremental sync** - Only send changed text, not full document
4. **Background processing** - Don't block UI thread
5. **Server pooling** - Reuse servers across files

### Testing Strategy

1. **Unit tests** - Test LSP message parsing
2. **Integration tests** - Test with mock LSP server
3. **Manual tests** - Test with real rust-analyzer, gopls
4. **Performance tests** - Measure latency, memory usage

---

## 2. Tree-sitter Integration

### Overview

Tree-sitter provides:
- **Fast, incremental parsing** - Only re-parse changed sections
- **Syntax highlighting** - Accurate, semantic highlighting
- **Code navigation** - AST-based navigation
- **Code folding** - Fold functions, blocks
- **Structural editing** - Select/move by syntax nodes
- **Error recovery** - Parse even with syntax errors

### Why Tree-sitter?

**Advantages over regex-based highlighting:**
- ✅ **Accurate** - Understands code structure, not just patterns
- ✅ **Fast** - Incremental parsing (1-2ms for typical edits)
- ✅ **Robust** - Handles incomplete/invalid code gracefully
- ✅ **Consistent** - Same highlighting as VSCode, Neovim, etc.

**Comparison:**
```
Regex:     const foo = "string";  // All one color
                                   // Can't distinguish types

Tree-sitter: const foo = "string";
             ^^^^^      ^^^^^^^^
             keyword    string
                   ^^^
                   variable
```

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    zblade Frontend                       │
│  ┌────────────────────────────────────────────────────┐ │
│  │  CodeMirror Editor                                 │ │
│  │  - Applies syntax highlighting                     │ │
│  │  - Uses tree-sitter tokens                         │ │
│  └────────────────────────────────────────────────────┘ │
│                          ↕ (events)                      │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Syntax Highlighter                                │ │
│  │  - Receives tokens from backend                    │ │
│  │  - Maps to theme colors                            │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
                          ↕ (Tauri commands)
┌─────────────────────────────────────────────────────────┐
│                    zblade Backend (Rust)                 │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Tree-sitter Manager                               │ │
│  │  - Maintains parse trees per file                  │ │
│  │  - Incremental re-parsing on edits                 │ │
│  │  - Extracts syntax tokens                          │ │
│  └────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Tree-sitter Parsers (Rust bindings)               │ │
│  │  - tree-sitter-rust                                │ │
│  │  - tree-sitter-go                                  │ │
│  │  - tree-sitter-typescript                          │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### Implementation Plan

#### Phase 1: Foundation (Week 1)

**Dependencies:**
```toml
# Cargo.toml
[dependencies]
tree-sitter = "0.20"
tree-sitter-rust = "0.20"
tree-sitter-go = "0.20"
tree-sitter-typescript = "0.20"
tree-sitter-python = "0.20"
```

**Backend:**
```rust
// src-tauri/src/syntax/mod.rs
pub struct SyntaxManager {
    parsers: HashMap<String, Parser>,  // language -> parser
    trees: HashMap<String, Tree>,      // file_path -> parse tree
}

impl SyntaxManager {
    pub fn parse_file(&mut self, language: &str, content: &str) -> Result<Vec<Token>, String> {
        let parser = self.get_parser(language)?;
        let tree = parser.parse(content, None)?;
        Ok(self.extract_tokens(&tree))
    }
    
    pub fn update_file(&mut self, 
        file_path: &str, 
        edit: &Edit
    ) -> Result<Vec<Token>, String> {
        // Incremental re-parse
        let old_tree = self.trees.get(file_path)?;
        let new_tree = parser.parse(new_content, Some(old_tree))?;
        self.trees.insert(file_path.to_string(), new_tree);
        Ok(self.extract_tokens(&new_tree))
    }
}

#[derive(Serialize)]
pub struct Token {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub token_type: String,  // "keyword", "function", "variable", etc.
}
```

**Tauri Commands:**
```rust
#[tauri::command]
async fn syntax_parse(
    language: String,
    file_path: String,
    content: String
) -> Result<Vec<Token>, String>

#[tauri::command]
async fn syntax_update(
    file_path: String,
    edit: EditInfo
) -> Result<Vec<Token>, String>
```

#### Phase 2: Highlighting (Week 2)

**Token Types (TextMate scopes):**
```rust
pub enum TokenType {
    Keyword,              // if, let, const
    Function,             // function names
    Variable,             // variable names
    Type,                 // struct, class names
    String,               // string literals
    Number,               // numeric literals
    Comment,              // comments
    Operator,             // +, -, *, /
    Punctuation,          // {, }, (, )
    Property,             // object.property
    Parameter,            // function parameters
    Constant,             // CONSTANT_NAME
}
```

**Frontend Integration:**
```typescript
// src/hooks/useSyntaxHighlighting.ts
export function useSyntaxHighlighting(filePath: string, language: string) {
  const [tokens, setTokens] = useState<Token[]>([]);
  
  const parseFile = async (content: string) => {
    const result = await invoke<Token[]>('syntax_parse', {
      language,
      filePath,
      content
    });
    setTokens(result);
  };
  
  const updateFile = async (edit: EditInfo) => {
    const result = await invoke<Token[]>('syntax_update', {
      filePath,
      edit
    });
    setTokens(result);
  };
  
  return { tokens, parseFile, updateFile };
}
```

#### Phase 3: Advanced Features (Week 3-4)

**Features:**
1. ✅ Code folding (fold functions, blocks)
2. ✅ Structural selection (select by syntax node)
3. ✅ AST-based navigation (jump to next function)
4. ✅ Syntax-aware search (find all function calls)

### Performance

**Benchmarks (typical file):**
- Initial parse: ~5-10ms for 1000 lines
- Incremental update: ~1-2ms for single edit
- Token extraction: ~1ms

**Optimization:**
- Parse in background thread
- Cache parse trees
- Only re-highlight visible range
- Debounce updates (50ms)

---

## 3. Theming System

### Overview

A robust theming system needs:
- **Syntax colors** - Code highlighting
- **UI colors** - Panels, buttons, borders
- **Semantic colors** - Errors, warnings, info
- **Dark/light variants**
- **Easy customization**

### Strategy: VSCode Theme Compatibility

**Recommendation: Adopt VSCode theme format**

**Why VSCode themes?**
- ✅ **Ecosystem** - 1000+ existing themes
- ✅ **Familiar** - Users already know them
- ✅ **Well-designed** - Proven format
- ✅ **Tooling** - Theme generators, converters
- ✅ **Documentation** - Extensive docs

**Format:**
```json
{
  "name": "Monokai Pro",
  "type": "dark",
  "colors": {
    "editor.background": "#2d2a2e",
    "editor.foreground": "#fcfcfa",
    "editorLineNumber.foreground": "#5b595c",
    "editorCursor.foreground": "#fcfcfa",
    "editor.selectionBackground": "#5b595c80"
  },
  "tokenColors": [
    {
      "scope": ["keyword", "storage.type"],
      "settings": {
        "foreground": "#ff6188"
      }
    },
    {
      "scope": ["string"],
      "settings": {
        "foreground": "#ffd866"
      }
    }
  ]
}
```

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    zblade Frontend                       │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Theme Provider (React Context)                    │ │
│  │  - Provides theme to all components                │ │
│  │  - Injects CSS variables                           │ │
│  └────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Components                                        │ │
│  │  - Use CSS variables: var(--editor-bg)            │ │
│  │  - Automatically update on theme change           │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
                          ↕ (Tauri commands)
┌─────────────────────────────────────────────────────────┐
│                    zblade Backend (Rust)                 │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Theme Manager                                     │ │
│  │  - Loads themes from disk                          │ │
│  │  - Parses VSCode theme format                      │ │
│  │  - Stores active theme                             │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### Implementation Plan

#### Phase 1: Theme Loading (Week 1)

**Backend:**
```rust
// src-tauri/src/theme/mod.rs
#[derive(Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub theme_type: ThemeType,  // Dark or Light
    pub colors: HashMap<String, String>,
    pub token_colors: Vec<TokenColor>,
}

#[derive(Serialize, Deserialize)]
pub struct TokenColor {
    pub scope: Vec<String>,
    pub settings: TokenSettings,
}

pub struct ThemeManager {
    themes: HashMap<String, Theme>,
    active_theme: String,
}

impl ThemeManager {
    pub fn load_themes(&mut self) -> Result<(), String> {
        // Load from ~/.zblade/themes/
        // Also bundle default themes
    }
    
    pub fn get_theme(&self, name: &str) -> Option<&Theme> {
        self.themes.get(name)
    }
}
```

**Tauri Commands:**
```rust
#[tauri::command]
async fn list_themes() -> Result<Vec<ThemeInfo>, String>

#[tauri::command]
async fn get_theme(name: String) -> Result<Theme, String>

#[tauri::command]
async fn set_theme(name: String) -> Result<(), String>

#[tauri::command]
async fn import_theme(path: String) -> Result<(), String>
```

#### Phase 2: Frontend Integration (Week 2)

**Theme Context:**
```typescript
// src/contexts/ThemeContext.tsx
interface ThemeContextType {
  theme: Theme;
  setTheme: (name: string) => void;
  availableThemes: ThemeInfo[];
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme | null>(null);
  
  useEffect(() => {
    // Load active theme on mount
    invoke<Theme>('get_theme', { name: 'default' }).then(setThemeState);
  }, []);
  
  useEffect(() => {
    if (!theme) return;
    
    // Inject CSS variables
    const root = document.documentElement;
    Object.entries(theme.colors).forEach(([key, value]) => {
      const cssVar = `--${key.replace('.', '-')}`;
      root.style.setProperty(cssVar, value);
    });
  }, [theme]);
  
  const setTheme = async (name: string) => {
    await invoke('set_theme', { name });
    const newTheme = await invoke<Theme>('get_theme', { name });
    setThemeState(newTheme);
  };
  
  return (
    <ThemeContext.Provider value={{ theme, setTheme, availableThemes }}>
      {children}
    </ThemeContext.Provider>
  );
}
```

**Usage in Components:**
```typescript
// Use CSS variables
<div style={{ 
  backgroundColor: 'var(--editor-background)',
  color: 'var(--editor-foreground)'
}}>
  Editor content
</div>

// Or with Tailwind
<div className="bg-[var(--editor-background)] text-[var(--editor-foreground)]">
  Editor content
</div>
```

#### Phase 3: Bundled Themes (Week 3)

**Include popular themes:**
1. **Monokai Pro** (dark)
2. **One Dark Pro** (dark)
3. **Dracula** (dark)
4. **GitHub Light** (light)
5. **Solarized Light** (light)
6. **Nord** (dark)
7. **Material Theme** (dark/light variants)

**Location:**
```
src-tauri/themes/
├── monokai-pro.json
├── one-dark-pro.json
├── dracula.json
├── github-light.json
└── ...
```

#### Phase 4: Theme Editor (Week 4)

**Features:**
- Visual theme editor
- Live preview
- Export to JSON
- Share themes

### CSS Variable Mapping

**VSCode → zblade CSS variables:**
```css
/* Editor */
--editor-background: editor.background
--editor-foreground: editor.foreground
--editor-line-number: editorLineNumber.foreground
--editor-cursor: editorCursor.foreground
--editor-selection: editor.selectionBackground

/* UI */
--sidebar-background: sideBar.background
--sidebar-foreground: sideBar.foreground
--panel-background: panel.background
--button-background: button.background
--button-foreground: button.foreground

/* Syntax */
--syntax-keyword: (from tokenColors)
--syntax-string: (from tokenColors)
--syntax-comment: (from tokenColors)
--syntax-function: (from tokenColors)
```

### Alternative: Custom Theme Format

**If NOT using VSCode themes:**

```json
{
  "name": "zblade Dark",
  "version": "1.0.0",
  "type": "dark",
  "author": "zblade team",
  "ui": {
    "background": "#1e1e1e",
    "foreground": "#d4d4d4",
    "primary": "#007acc",
    "secondary": "#3e3e42",
    "accent": "#0098ff",
    "border": "#454545",
    "error": "#f48771",
    "warning": "#cca700",
    "success": "#89d185",
    "info": "#75beff"
  },
  "editor": {
    "background": "#1e1e1e",
    "foreground": "#d4d4d4",
    "lineNumber": "#858585",
    "cursor": "#aeafad",
    "selection": "#264f78",
    "activeLineBackground": "#282828"
  },
  "syntax": {
    "keyword": "#569cd6",
    "string": "#ce9178",
    "number": "#b5cea8",
    "comment": "#6a9955",
    "function": "#dcdcaa",
    "variable": "#9cdcfe",
    "type": "#4ec9b0",
    "operator": "#d4d4d4",
    "constant": "#4fc1ff"
  }
}
```

**Pros:**
- Simpler format
- Easier to understand
- Custom to zblade's needs

**Cons:**
- No existing themes
- Users must create from scratch
- Smaller ecosystem

**Recommendation:** Start with VSCode compatibility, add custom format later if needed.

---

## 4. AI-LSP Integration (The "Magic" Feature)

### Overview

**This is zblade's killer feature**: LSP diagnostics automatically feed into the AI, enabling proactive error correction.

**The Flow:**
```
LSP detects error → Backend analyzes → AI proposes fix → User approves → Error fixed
```

**What makes this special:**
- ✅ Errors fixed automatically (with approval)
- ✅ Learn from rejections
- ✅ Batch related fixes
- ✅ Confidence scoring
- ✅ Context-aware fixes

### Architecture

```
LSP Server → LSP Manager → AI Diagnostic Analyzer → AI Workflow → propose-edit
```

### Implementation Phases

#### Phase 1: Basic Integration (Week 1-2)
- LSP diagnostics trigger AI analysis
- AI proposes fixes for simple errors (missing imports, typos)
- User approves/rejects fixes
- Track fix success rate

#### Phase 2: Smart Fixes (Week 3-4)
- Batch related fixes
- Confidence scoring (High/Medium/Low)
- Context-aware fixes (full file context)
- Learning from rejections

#### Phase 3: Advanced (Week 5-6)
- Auto-apply high-confidence fixes (opt-in)
- Fix suggestions in hover
- "Fix all similar" action
- AI explains why fix works

#### Phase 4: Intelligence (Week 7-8)
- Predict errors before LSP reports them
- Suggest refactorings to prevent errors
- Learn project-specific patterns
- Multi-file fixes

### New Events

```rust
// Diagnostic received from LSP
"diagnostic-received" -> DiagnosticReceivedPayload

// AI analyzing diagnostic
"ai-analyzing-diagnostic" -> AiAnalyzingDiagnosticPayload

// AI fix applied
"ai-fix-applied" -> AiFixAppliedPayload

// AI fix rejected
"ai-fix-rejected" -> AiFixRejectedPayload
```

### User Settings

```json
{
  "ai": {
    "autoFix": {
      "enabled": true,
      "autoApplyHighConfidence": false,
      "fixErrors": true,
      "fixWarnings": false,
      "excludeErrorCodes": ["E0277"],
      "batchRelatedFixes": true
    }
  }
}
```

### Competitive Advantages

**vs Windsurf:**
- Tighter integration (we control both LSP and AI)
- Batch fixes for related errors
- Learning from user preferences

**vs GitHub Copilot:**
- Error-driven, not just completion
- Context from LSP (type info, diagnostics)
- Fix verification (LSP confirms fix works)

**vs Cursor:**
- Open source AI backend (zcoderd)
- Full control over prompts
- Self-hosted option for privacy

### Performance

- **Debouncing**: Wait 2s after last diagnostic before sending to AI
- **Caching**: Cache fixes for identical errors
- **Rate Limiting**: Max requests per minute to zcoderd

### Privacy

**Sent to AI:**
- Error message and code
- Surrounding code (5-10 lines)
- File type

**NOT sent:**
- Full file contents (unless needed)
- Secrets/credentials (filtered)
- Other workspace files

**See `AI_LSP_INTEGRATION.md` for complete details.**

---

## Integration Timeline

### Quarter 1 (Months 1-3)
- ✅ LSP Integration (Phases 1-2)
- ✅ Tree-sitter Integration (Phases 1-2)
- ✅ Theming System (Phases 1-2)
- ✅ AI-LSP Integration (Phase 1)

### Quarter 2 (Months 4-6)
- ✅ LSP Advanced Features (Phase 3)
- ✅ Tree-sitter Advanced Features (Phase 3)
- ✅ Bundled Themes (Phase 3)
- ✅ AI-LSP Smart Fixes (Phase 2)
- ✅ Polish and optimization

### Quarter 3 (Months 7-9)
- ✅ LSP for more languages
- ✅ Theme editor
- ✅ AI-LSP Advanced Features (Phases 3-4)
- ✅ User feedback and refinement

---

## Success Metrics

**LSP:**
- ✅ Completion latency < 100ms
- ✅ Diagnostics update < 500ms
- ✅ Support for 5+ languages

**Tree-sitter:**
- ✅ Parse time < 10ms for typical file
- ✅ Incremental update < 2ms
- ✅ Accurate highlighting for 10+ languages

**Theming:**
- ✅ Theme switch < 100ms
- ✅ 10+ bundled themes
- ✅ VSCode theme import works

**AI-LSP:**
- ✅ Fix proposal latency < 2s
- ✅ Fix acceptance rate > 70%
- ✅ Support for 10+ error types per language
- ✅ Zero false positives (user always approves)

---

## Notes

- All three features are **interconnected**:
  - LSP provides semantic tokens → Tree-sitter provides syntax tokens → Theme colors both
  - Tree-sitter AST → LSP uses for navigation
  - Theme applies to both LSP diagnostics and tree-sitter highlighting

- **Start with one language** (Rust) for each feature, then expand
- **Prioritize correctness over performance** initially
- **User feedback** is critical - iterate based on real usage

---

## Resources

**LSP:**
- Spec: https://microsoft.github.io/language-server-protocol/
- Rust crate: https://github.com/rust-lang/lsp-types

**Tree-sitter:**
- Website: https://tree-sitter.github.io/
- Rust bindings: https://github.com/tree-sitter/tree-sitter/tree/master/lib/binding_rust

**VSCode Themes:**
- Theme format: https://code.visualstudio.com/api/extension-guides/color-theme
- Theme gallery: https://vscodethemes.com/
