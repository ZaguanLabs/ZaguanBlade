# Zaguán Blade

**The AI-Native Code Editor Built on Rust.**

Zaguán Blade is a high-performance AI code editor that takes a fundamentally different approach from VSCode forks. While others bolt AI onto existing editors, we built from the ground up with AI as the foundation—and Rust as the engine.

> [!WARNING]
> **Alpha Release (v0.0.4-alpha)**
> 
> Zaguán Blade is in active alpha development. It's functional and good enough for basic daily work, but expect bugs and incomplete features.
> 
> **What's Working:**
> - ✅ AI chat with full project context
> - ✅ Agentic tool execution (file operations, terminal commands)
> - ✅ Real-time file watching and auto-reload
> - ✅ Uncommitted changes tracking with accept/reject
> - ✅ History snapshots and undo system
> - ✅ Tree-sitter parsing and symbol indexing
> - ✅ ZLP (Zaguán Language Protocol) - AI-first code analysis
> - ✅ Multi-language syntax highlighting (CodeMirror 6)
> - ✅ Integrated terminal
> - ✅ Git integration
> 
> **Known Limitations:**
> - ⚠️ Diff views are being rebuilt (changes apply immediately to disk)
> - ⚠️ ZLP (Zaguán Language Protocol) in development - AI-first alternative to LSP
> - ⚠️ Some UI polish needed
> 
> **Stability:** Experimental but usable for real work.

---

## Why build another AI editor?

Zaguán Blade isn't just another AI editor. It's a combination of a code editor and an AI system backend. Together they create a whole and I had two goals in mind when I started this project:

1. **AI-Native Workflow**: Deeply integrated AI that understands your project context.
2. **Save Money**: Vibe Coding sends a lot of data to the providers and they charge a lot for it. The server I created does its best to send only what is relevant while also making sure that the model has enough context to understand your project.

These 2 systems work together to create a whole that is much more than the sum of its parts. I spent a lot of time getting the server to work well and borrowed a lot of knowledge from many other open source projects like Cline, Roo-Code, OpenCode, Codex, Gemini-CLI, Qwen-Code, and many others.

### Active Development

Zaguán Blade is currently in active development. We are working on many new features and improvements and things may be unstable at times while I update the server. I will try my very best to keep the updates regular and give a heads up, but that's not a guarantee during this phase of development.

There are many things that I've planned for both Zaguán Blade and the server too numerous to list here.

The server and the system prompts are relatively opinionated tailored to my preferences and the way I like to work.

I'm also planning on updating the GUI that emphasizes more the AI-first approach and workflow. I was mostly inspired by the many VSCode forks out there like Windsurf, Cursor et al during the initial development just to get something working.

---

## What Makes Zaguán Blade Different?

### 1. **Rust-First Architecture**

Unlike VSCode forks (Cursor, Windsurf, etc.) that run heavy processing in JavaScript, Zaguán Blade does **everything performance-critical in Rust**:

- **Tree-sitter parsing**: Native Rust, not WASM → **10x faster** (5ms vs 50ms)
- **Symbol indexing**: SQLite in Rust with full-text search → **10x faster**
- **File operations**: Native async I/O with Tokio → **No main thread blocking**
- **Context assembly**: Parallel processing in Rust → **10x faster** than JS

**Result:** 5x less memory usage, 10x faster operations, smoother UI.

### 2. **"File on Disk is Truth" Paradigm**

Most AI editors maintain complex virtual buffers and preview states. Zaguán Blade is simpler:

- AI writes changes **directly to disk** (with history snapshots)
- File watcher triggers automatic reload
- Accept = Keep the change (already on disk)
- Reject = Revert from history snapshot

**Why this matters:** No state synchronization bugs, no "preview vs actual" confusion, instant feedback.

### 3. **Dual Protocol Architecture**

Zaguán Blade uses two distinct protocols:

**Blade Protocol** - Communication between Blade and zcoderd (AI backend):
- WebSocket-based streaming
- Handles AI chat, tool execution, context assembly
- Server-side code analysis and validation

**BCP (Blade Change Protocol)** - Internal IPC within Blade:
- **Versioned & extensible**: Semantic versioning with compatibility checks
- **Domain-based**: Chat, Editor, File, Workflow, Terminal, History, System
- **Intent/Event model**: Clear causality tracking with UUIDs
- **Idempotency**: Prevents duplicate operations on retry
- Unified dispatcher pattern with single `dispatch()` command

**Not just JSON-RPC.** Purpose-built for AI-native workflows.

### 4. **Intelligent Context Assembly**

The backend intelligently assembles code context:

- **Symbol-based selection**: Includes related definitions, usages, types
- **Token budget management**: Fits within model context limits
- **Semantic relationships**: Uses tree-sitter for smart extraction
- **Parallel processing**: Multiple files processed concurrently

**Result:** AI gets exactly what it needs, nothing more. Lower costs, better results.

### 5. **Agentic Loop with Guardrails**

Built-in protection against common AI failure modes:

- **Loop detection**: Prevents repetitive tool calls
- **Stagnation detection**: Stops when AI makes no progress
- **Tool spam prevention**: Limits identical operations
- **Parallel read optimization**: Multiple file reads execute concurrently

**Why this matters:** Saves tokens, prevents runaway costs, faster execution.

### 6. **History & Uncommitted Changes System**

Every AI change is tracked:

- **Automatic snapshots** before any modification
- **Uncommitted changes panel** shows all pending changes
- **Per-file accept/reject** with diff preview
- **Batch operations**: Accept all / Reject all
- **Full undo history** with group operations

**Currently working:** Changes apply immediately, tracking system operational.

### 7. **Native Performance**

Built with Tauri v2 (not Electron):

- **Compact bundles**: 12MB (.deb/.rpm), 95MB (AppImage with runtime)
- **Lower memory**: 50-100MB vs 200-500MB for Electron apps
- **Native speed**: Rust backend, no VM overhead
- **Better startup**: <100ms to first interaction
- **Efficient**: No Chromium overhead, uses system WebView

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Frontend (React + Vite)                  │
│  • CodeMirror 6 editor                                      │
│  • Minimal UI logic (rendering only)                        │
│  • Dispatches intents via BCP                               │
│  • Listens for events                                       │
└─────────────────────────────────────────────────────────────┘
                            ↕ Tauri IPC
┌─────────────────────────────────────────────────────────────┐
│                   Backend (Rust + Tokio)                    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Blade Protocol Dispatcher                          │    │
│  │  • Intent routing                                   │    │
│  │  • Event emission                                   │    │
│  │  • Causality tracking                               │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  AI Workflow Engine                                 │    │
│  │  • Agentic loop management                          │    │
│  │  • Tool execution                                   │    │
│  │  • Loop detection                                   │    │
│  │  • Context assembly                                 │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Core Services                                      │    │
│  │  • Tree-sitter parser (native)                      │    │
│  │  • Symbol index (SQLite)                            │    │
│  │  • History service (snapshots)                      │    │
│  │  • Uncommitted changes tracker                      │    │
│  │  • File watcher (notify)                            │    │
│  │  • Git operations                                   │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
                            ↕ WebSocket
┌─────────────────────────────────────────────────────────────┐
│                    Zaguán AI Backend                        │
│  • Hosted AI service                                        │
│  • Context optimization                                     │
│  • Multi-model support                                      │
│  • Cost optimization                                        │
└─────────────────────────────────────────────────────────────┘
```

---

## Tech Stack

### Backend (Rust)
- **Tauri v2**: Native desktop framework
- **Tokio**: Async runtime for parallel processing
- **Tree-sitter**: Native code parsing (Rust, TypeScript, JavaScript, Python)
- **rusqlite**: Symbol indexing and full-text search
- **diffy**: Unified diff generation
- **notify**: File system watching
- **reqwest**: HTTP client for AI backend
- **tokio-tungstenite**: WebSocket client

### Frontend (TypeScript/React)
- **React 19**: UI framework
- **CodeMirror 6**: Lightweight code editor
- **Vite**: Build tool
- **TailwindCSS 4**: Styling
- **xterm.js**: Terminal emulator
- **react-markdown**: Markdown rendering

---

## Requirement: Zaguán AI Subscription

Zaguán Blade is powered by our hosted AI backend. To use the AI features (Chat, Code Generation, Auto-fix), you **must have an active subscription**.

👉 **[Get a Subscription at ZaguanAI.com](https://zaguanai.com/pricing)**

Without a subscription and a valid API Key, Zaguán Blade functions as a standard (albeit very nice) text editor with syntax highlighting, file management, and terminal integration.

---

## Installation

### Pre-built Binaries

We provide pre-built binaries for:
- **Windows**: `.msi`, `.exe`
- **macOS**: `.dmg`, `.app` (Intel & Apple Silicon)
- **Linux**: `.AppImage`, `.deb`, `.rpm`

Download from **[Releases](https://github.com/ZaguanLabs/ZaguanBlade/releases)**

### Building from Source

Requirements:
- Rust 1.70+
- Node.js 18+ (we use Bun for package management)
- Platform-specific dependencies (see build guide)

```bash
git clone https://github.com/ZaguanLabs/ZaguanBlade.git
cd ZaguanBlade
bun install
bun run tauri build
```

For detailed instructions, see **[Build Guide](docs/BUILD_FROM_SOURCE.md)**.

---

## Quick Start

1. **Install** Zaguán Blade from releases
2. **Launch** the application
3. **Open Settings** (gear icon, bottom-left)
4. **Add your API Key** (Account tab)
5. **Open a project** (File → Open Folder)
6. **Start chatting** with the AI in the right panel

The AI can:
- Read and edit files
- Run terminal commands (with approval)
- Search your codebase
- Explain code
- Refactor and debug
- Create new files and directories

---

## Key Features in Detail

### AI Chat with Full Context
- AI sees your active file, cursor position, and open files
- Automatic context assembly based on relevance
- Multi-turn conversations with tool execution
- Streaming responses

### Agentic Tool Execution
- **File operations**: Read, write, edit, delete, move, copy
- **Terminal commands**: Run with approval, background execution
- **Search**: Grep, file search, symbol search
- **Git**: Status, diff, commit, branch operations
- **Web search**: Research capabilities (via AI backend)

### Uncommitted Changes System
- All AI changes tracked automatically
- View diffs for each change
- Accept or reject per file
- Batch accept/reject all changes
- Full history with snapshots

### Integrated Terminal
- Multiple terminal instances
- Persistent across sessions
- AI can read terminal output
- Command approval workflow

### Symbol Indexing
- Tree-sitter based parsing
- SQLite full-text search
- Fast symbol lookup
- Supports: Rust, TypeScript, JavaScript, Python, TSX, JSX

### Git Integration
- Visual diff viewer
- Commit history
- Branch management
- Stage/unstage files
- Integrated with AI workflow

---

## Comparison with Other AI Editors

| Feature | Zaguán Blade | Cursor | Windsurf | Cline |
|---------|--------------|--------|----------|-------|
| **Architecture** | Rust-first | VSCode fork | VSCode fork | VSCode extension |
| **Performance** | Native (Tauri) | Electron | Electron | VSCode |
| **Parsing** | Tree-sitter (Rust) | Tree-sitter (WASM) | Tree-sitter (WASM) | VSCode API |
| **Memory Usage** | ~50-100MB | ~300-500MB | ~300-500MB | ~200-400MB |
| **Editor** | CodeMirror 6 | Monaco | Monaco | Monaco |
| **Bundle Size** | 12-95MB | ~150MB+ | ~150MB+ | Extension |
| **Change Model** | Disk-first | Buffer-based | Buffer-based | Buffer-based |
| **Context Assembly** | Rust (parallel) | JavaScript | JavaScript | JavaScript |
| **Custom Protocol** | BCP (binary) | JSON-RPC | JSON-RPC | JSON-RPC |
| **Open Source** | ✅ MIT | ❌ Proprietary | ❌ Proprietary | ✅ Apache 2.0 |
| **Self-hostable** | ✅ (editor only) | ❌ | ❌ | ✅ |

**Our advantage:** Rust performance + custom architecture = 10x faster operations, 5x less memory.

---

## Contributing

We welcome contributions! Zaguán Blade is MIT licensed and open source.

**Ways to contribute:**
- 🐛 **Report bugs**: [GitHub Issues](https://github.com/ZaguanLabs/ZaguanBlade/issues)
- 💡 **Suggest features**: [GitHub Discussions](https://github.com/ZaguanLabs/ZaguanBlade/discussions)
- 🔧 **Submit PRs**: Check open issues or propose new features
- 📖 **Improve docs**: Documentation PRs always welcome
- 🌍 **Translations**: Help us support more languages

**Development setup:**
```bash
git clone https://github.com/ZaguanLabs/ZaguanBlade.git
cd ZaguanBlade
bun install
bun run tauri dev
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines (coming soon).

---

## Documentation

- **[User Guide](docs/USER_GUIDE.md)**: Getting started and features
- **[Build Guide](docs/BUILD_FROM_SOURCE.md)**: Compilation instructions
- **Internal Docs**: Architecture, protocols, and RFCs in `docs/internal/`

---

## Community & Support

- **GitHub Issues**: Bug reports and feature requests
- **GitHub Discussions**: Questions, ideas, and community chat
- **Website**: [zaguanai.com](https://zaguanai.com)
- **Email**: support@zaguanai.com

---

## Acknowledgments

Zaguán Blade was inspired by and learned from many excellent open source projects:

- **Cline**: Agentic workflow patterns
- **Roo-Code/Kilocode**: Diff handling approaches
- **Codex**: Rust TUI architecture
- **Cursor & Windsurf**: AI-first editor UX
- **VSCode**: Editor standards and conventions
- **Tauri**: Native desktop framework
- **CodeMirror**: Lightweight editor foundation

Thank you to the open source community for building the foundation we stand on.

---

## License

**MIT License** - See [LICENSE](LICENSE) for details.

Zaguán Blade is free and open source. The hosted AI backend requires a subscription, but the editor itself is yours to use, modify, and distribute.

---

## Project Status

**Current Version:** v0.0.4-alpha  
**Status:** Active Development  
**Stability:** Alpha (usable, but expect bugs)  
**License:** MIT  
**Language:** Rust (backend) + TypeScript (frontend)

**Star us on GitHub if you find this project interesting!** ⭐
