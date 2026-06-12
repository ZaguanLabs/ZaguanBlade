# Zaguán Blade

**An open source AI engineering environment for local AI and hosted Zaguán workflows.**

Zaguán Blade is an open source desktop code editor built for AI-assisted software work: project exploration, codebase investigation, file editing, terminal workflows, debugging, and validation.

Blade can be used with local AI models, making it possible to try the editor and work privately without a subscription.

The full Zaguán experience is powered by the proprietary **Zaguán Coder Daemon**, our hosted AI backend. Most advanced AI workflows and power features require an active subscription.

---

## What Zaguán Blade Is

Zaguán Blade is the open source desktop surface for working with AI inside a real development workspace.

It gives AI controlled access to:

- files on disk
- editor state
- terminal commands
- project structure
- symbol search
- history snapshots
- uncommitted changes
- validation feedback

The goal is not just to chat with a model. The goal is to help AI investigate, change, and validate software work in a way developers can inspect and control.

---

## Open Source Editor, Hosted Power Features

Blade itself is open source and MIT licensed. You can build it, inspect it, modify it, and use it with local AI.

The advanced Zaguán workflows are powered by **Zaguán Coder Daemon**, which is proprietary and subscription-backed.

There are currently no plans to support on-premise Zaguán Coder Daemon deployments.

---

## Local AI

Local AI is a first-class part of Blade.

You can use Blade with local models to try the experience, explore projects, ask questions, and perform lighter AI-assisted development tasks without a subscription.

Local model quality varies, and some workflows require stronger hosted models, larger context, or backend orchestration through Zaguán Coder Daemon.

For the best Local AI setup and usage guidance, see the documentation:

**[zblade.dev/docs](https://zblade.dev/docs/)**

---

## Zaguán Subscription

A Zaguán subscription connects Blade to the hosted **Zaguán Coder Daemon**.

This unlocks the intended full experience, including most power features:

- stronger hosted model workflows
- managed agentic orchestration
- deeper codebase investigation
- better context handling
- web-backed research where available
- advanced tool workflows
- future commercial workflows such as security review, blindspot discovery, and traceable engineering review

Blade remains open source. Zaguán Coder Daemon is the commercial backend.

👉 **[Get a subscription at ZaguanAI.com](https://zaguanai.com/pricing)**

---

## What Makes Blade Different?

### 1. **Evidence-Grounded AI Workflow**

Most AI coding tools optimize for producing an answer from a prompt. Zaguán Blade is built around a more controlled loop:

1. Inspect the workspace.
2. Gather relevant evidence.
3. Run approved tools.
4. Make or propose changes.
5. Track what changed.
6. Preserve enough context to explain what happened.

This matters for real engineering work where a confident answer is not enough.

### 2. **Rust-First Architecture**

Unlike VSCode forks (Cursor, Windsurf, etc.) that run heavy processing in JavaScript, Zaguán Blade does **everything performance-critical in Rust**:

- **Tree-sitter parsing**: Native Rust, not WASM → **10x faster** (5ms vs 50ms)
- **Symbol indexing**: SQLite in Rust with full-text search → **10x faster**
- **File operations**: Native async I/O with Tokio → **No main thread blocking**
- **Context assembly**: Parallel processing in Rust → **10x faster** than JS

**Result:** 5x less memory usage, 10x faster operations, smoother UI.

### 3. **"File on Disk is Truth" Paradigm**

Most AI editors maintain complex virtual buffers and preview states. Zaguán Blade is simpler:

- AI writes changes **directly to disk** (with history snapshots)
- File watcher triggers automatic reload
- Accept = Keep the change (already on disk)
- Reject = Revert from history snapshot

**Why this matters:** No state synchronization bugs, no "preview vs actual" confusion, instant feedback.

### 4. **Dual Protocol Architecture**

Zaguán Blade uses two distinct protocols:

**Blade Protocol** - Communication between Blade and Zaguán Coder Daemon:
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

### 5. **Intelligent Context Assembly**

The backend intelligently assembles code context:

- **Symbol-based selection**: Includes related definitions, usages, types
- **Token budget management**: Fits within model context limits
- **Semantic relationships**: Uses tree-sitter for smart extraction
- **Parallel processing**: Multiple files processed concurrently

**Result:** AI gets exactly what it needs, nothing more. Lower costs, better results.

### 6. **Agentic Loop with Guardrails**

Built-in protection against common AI failure modes:

- **Loop detection**: Prevents repetitive tool calls
- **Stagnation detection**: Stops when AI makes no progress
- **Tool spam prevention**: Limits identical operations
- **Parallel read optimization**: Multiple file reads execute concurrently

**Why this matters:** Saves tokens, prevents runaway costs, faster execution.

### 7. **History & Uncommitted Changes System**

Every AI change is tracked:

- **Automatic snapshots** before any modification
- **Uncommitted changes panel** shows all pending changes
- **Per-file accept/reject** with diff preview
- **Batch operations**: Accept all / Reject all
- **Full undo history** with group operations

**Currently working:** Changes apply immediately, tracking system operational.

### 8. **Native Performance**

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
│                  Zaguán Coder Daemon                        │
│  • Hosted proprietary AI backend                            │
│  • Context optimization                                     │
│  • Multi-model support                                      │
│  • Tool orchestration                                       │
│  • Subscription-backed power features                       │
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
- **reqwest**: HTTP client for Zaguán Coder Daemon
- **tokio-tungstenite**: WebSocket client

### Frontend (TypeScript/React)
- **React 19**: UI framework
- **CodeMirror 6**: Lightweight code editor
- **Vite**: Build tool
- **TailwindCSS 4**: Styling
- **xterm.js**: Terminal emulator
- **react-markdown**: Markdown rendering

---

## Installation

### Pre-built Binaries

We provide pre-built binaries for:
- **Windows**: `.msi`, `.exe`
- **macOS**: `.dmg`, `.app` (Apple Silicon)
- **Linux**: `.AppImage`, `.deb`, `.rpm`

Download from **[Releases](https://github.com/ZaguanLabs/ZaguanBlade/releases)**

### Building from Source

Requirements:
- Rust 1.70+
- Bun 1.3+
- Node.js 20.19+ or 22.12+ if you want to run Vite tooling directly without Bun
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
3. **Open a project** (File → Open Folder)
4. **Choose your AI setup**
   - Use Local AI to try Blade without a subscription
   - Add your Zaguán API key for the hosted Zaguán Coder Daemon experience
5. **Start chatting** with the AI in the right panel

Blade can read and edit files, run approved terminal commands, search your codebase, explain code, refactor, debug, and create new files or directories.

Local model capability varies. Most power features require a Zaguán subscription. See **[zblade.dev/docs](https://zblade.dev/docs/)** for setup and usage guidance.

---

## Capability Summary

- **AI chat** with active-file, cursor, open-file, and workspace context
- **Tool execution** for file operations, approved terminal commands, search, Git, and hosted web-backed research where available
- **Change review** with tracked uncommitted changes, diffs, accept/reject controls, and history snapshots
- **Integrated terminal** with multiple sessions, persistence, and command approval workflows
- **Symbol indexing** for Rust, TypeScript, JavaScript, Python, TSX, and JSX
- **Git integration** for diffs, history, branches, staging, and AI-assisted workflows

---

## Project Philosophy

Zaguán Blade is not trying to be a VS Code fork with a chat panel.

The project is built around a few practical beliefs:

- **The editor should be open source.** Developers should be able to inspect, modify, and build the desktop environment they use.
- **Local AI should be available.** Users should be able to try Blade and run useful workflows without subscribing first.
- **Files on disk are the source of truth.** AI changes should be visible to the filesystem, Git, tests, and other tools immediately.
- **Power features need orchestration.** The deepest workflows require Zaguán Coder Daemon, stronger models, hosted coordination, and subscription-backed infrastructure.
- **Engineering work needs evidence.** The AI should inspect, search, run tools, validate, and leave a trail instead of simply producing confident text.

---

## Contributing

We welcome contributions! Zaguán Blade is MIT licensed and open source.

**Ways to contribute:**
- 🐛 **Report bugs**: [GitHub Issues](https://github.com/ZaguanLabs/ZaguanBlade/issues)
- 💡 **Suggest features**: [GitHub Discussions](https://github.com/ZaguanLabs/ZaguanBlade/discussions)
- 🔧 **Submit PRs**: Check open issues or propose new features
- 📖 **Improve docs**: Documentation PRs always welcome
- 🌍 **Translations**: Help us support more languages

**Development setup:** Follow the [Building from Source](#building-from-source) instructions, then run the dev server with `bun run tauri dev`.

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines (coming soon).

---

## Documentation

- **[Online Docs](https://zblade.dev/docs/)**: Local AI setup, usage guidance, and workflow documentation
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

Zaguán Blade is free and open source. The hosted Zaguán Coder Daemon requires a subscription, but the editor itself is yours to use, modify, and distribute.

---

## Project Status

**Current Version:** v0.8.1
**Status:** Active Development  
**Stability:** Near-stable (very close to production-ready)  
**License:** MIT  
**Language:** Rust (backend) + TypeScript (frontend)  
**Changelog:** [zblade.dev/changelog](https://zblade.dev/changelog/)

**Star us on GitHub if you find this project interesting!** ⭐
