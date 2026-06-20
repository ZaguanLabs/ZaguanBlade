# Zaguán Blade User Guide

Zaguán Blade is an open source desktop code editor for AI-assisted engineering work. It can run with local AI providers, and it can connect to the hosted Zaguán Coder Daemon for the full subscription-backed workflow.

## 1. What Blade Does

Blade is built around an inspect, change, and validate loop:

- It can use editor state, open files, cursor position, selected paths, project structure, symbol indexes, terminal output, and uncommitted changes as context.
- It can propose or apply file edits, run approved commands, inspect the workspace, and help validate changes.
- AI edits are tracked so you can review, accept, reject, or undo the resulting file changes.

## 2. Getting Started

### Local AI

You can use Blade without a Zaguán subscription by enabling a local provider:

1. Open **Settings**.
2. Go to **Local AI**.
3. Enable one or more providers:
   - **Ollama (Local)** at `http://localhost:11434`
   - **Ollama Cloud** with an Ollama API key
   - **OpenAI-compatible Server** for llama.cpp, LocalAI, vLLM, and similar servers
4. Click **Test Connection**.
5. Click **Refresh Models**.
6. Save settings and pick the model from the chat model picker.

Local model quality and tool-calling reliability depend on the model and server. Local providers do not have built-in web fetch or research.

### Zaguán Subscription

A Zaguán subscription connects Blade to the hosted Zaguán Coder Daemon.

1. Subscribe at [ZaguanAI.com](https://zaguanai.com/pricing).
2. Open **Settings → Account**.
3. Paste your Zaguán API key.
4. Save settings.

Hosted models appear in the model picker when the key is valid. Subscription-backed workflows can use stronger hosted models, managed orchestration, richer context handling, and hosted research features where available.

### First Project Setup

When you open a project for the first time, Blade asks where to store conversation history:

- **Local Storage** stores conversations and local artifacts under `.zblade/` in the project.
- **Server Storage** stores conversations on Zaguán servers for faster retrieval and sync.

Storage mode controls conversation persistence. Model selection controls where AI inference runs.

Blade also keeps project settings, indexes, cache files, and history artifacts under `.zblade/`. The app ensures `.zblade` is ignored by the workspace Git repository.

## 3. Interface

### App Bar

The app bar contains the file menu, window controls, fullscreen handling, and project/app actions.

### Activity Bar

- **Explorer**: browse and manage files.
- **Git**: inspect changes, stage/unstage, commit, push, pull, fetch, and generate commit messages with AI.
- **File History**: view snapshots for the active file and revert when needed.
- **Settings**: configure account, local AI, storage, context, remote control, appearance, language, and app details.

### Editor

The editor is based on CodeMirror 6 and supports syntax highlighting for Rust, TypeScript, JavaScript, Python, Go, C/C++, HTML, CSS, JSON, YAML, PHP, Markdown, and related formats.

Markdown files can switch between edit and view modes with `Ctrl+E`.

### Chat Panel

The chat panel includes:

- **Code / Plan mode** toggle.
- Model picker with hosted, Ollama, and Local Server sections.
- Image/screenshot attachment controls when the selected model supports images.
- `@` suggestions for workspace paths and hosted commands such as `@web` and `@research`.
- Queued request handling while the assistant is already responding.
- Inline command approval cards and tool progress.

### Terminal

Blade includes integrated terminals with copy, paste, search, split, clear, and shell selection controls. AI-triggered `run_command` requests require approval unless project YOLO mode is enabled.

## 4. Keyboard Shortcuts

### App and Tabs

| Shortcut | Action |
|----------|--------|
| `F11` | Toggle fullscreen |
| `Ctrl+W` | Close current tab |
| `Ctrl+Tab` | Cycle to next tab |
| `Ctrl+Shift+Tab` | Cycle to previous tab |
| `Escape` | Close open modal, picker, or menu |

### Files and Editor

| Shortcut | Action |
|----------|--------|
| `Ctrl+N` | New file in the explorer |
| `Ctrl+Shift+N` | New folder in the explorer |
| `Ctrl+O` | Open folder from the file menu |
| `Ctrl+S` | Save active file |
| `Ctrl+Shift+S` | Save as |
| `Ctrl+Z` | Undo |
| `Ctrl+Shift+Z` | Redo |
| `Ctrl+F` | Find in file |
| `Ctrl+X` / `Ctrl+C` / `Ctrl+V` | Cut, copy, paste |
| `F2` | Rename symbol from the editor context menu |
| `Ctrl+E` | Toggle Markdown edit/view mode |

### Chat Composer

| Shortcut | Action |
|----------|--------|
| `Enter` | Send message |
| `Shift+Enter` | Insert newline |
| `@` | Open command/path suggestions |
| `Tab` or `Enter` | Accept active suggestion |
| `Escape` | Close suggestions |
| `Arrow Up` / `Arrow Down` | Navigate message history when the cursor is at the start/end of the composer |

### Terminal

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+C` | Copy terminal selection |
| `Ctrl+Shift+V` | Paste into terminal |
| `Ctrl+F` | Find in terminal |

## 5. Working With AI

Use **Plan** mode when you want investigation and a concrete plan before implementation. Use **Code** mode when you want the assistant to implement and validate changes.

Useful request patterns:

- "Explain the active file."
- "Find where this symbol is used."
- "Plan the safest way to refactor this component."
- "Run the relevant tests and fix the failure."
- "Use `@src/path/to/file.ts` as context."

When Blade has local indexes available, the assistant can use symbol search, file ranges, semantic anchors, impact analysis, and workspace structure rather than reading the whole project.

## 6. Reviewing AI Changes

AI file changes are written to disk with a history snapshot and tracked as uncommitted Blade changes.

- **Accept** keeps the current file contents.
- **Reject** restores the snapshot captured before the AI change.
- **Accept All** and **Reject All** operate on the current pending change set.
- File tabs show when AI edits are pending or unread.

The Git panel remains the source of truth for repository-level commits and staging.

## 7. Settings

### Configuration

- Theme
- Editor and chat text size
- Interface language
- **YOLO mode**, which auto-approves `run_command` requests for the current project only

### Account

- Zaguán API key
- Links to subscription management or pricing

### Local AI

- Ollama local URL
- Ollama Cloud API key
- OpenAI-compatible server URL
- Test connection and refresh model actions

### Storage

- Local or server conversation storage
- Metadata sync for local storage
- Context cache toggle and cache size

### Context

Available when a workspace is open:

- Max context tokens from 2K to 32K
- Context compression toggle
- Remote or local compression model
- Whether files matched by `.gitignore` may be included in AI context
- Existing `AGENTS.md` files are loaded as workspace instructions. Blade reads the root file and any nested files that apply to the active or relevant
  paths, and supports local Markdown includes such as `@workflow.md` or `@docs/workflow.md`.

### Remote

Remote control setup for connecting to Blade while it is running on your computer.

### About

Version, runtime, engine, mode, website, GitHub, and support links.

## 8. Screenshots and Images

Use the **Add** button in the composer toolbar to:

- Capture a window.
- Select a window and crop a region.
- Upload an image from disk.
- Paste images directly into the composer.

After capture, Blade shows a preview and annotation editor before attaching the image.

Image attachments require a model that supports images. They are disabled when only local models are available and for known unsupported models such as GLM.

### Linux and X11 Notes

On X11 desktops, capture is limited to windows visible on the current workspace. Covered windows can capture as black unless a compositor such as `picom` is running. Bring the target window forward before capture. Wayland, macOS, and composited desktops generally avoid these X11 limitations.

## 9. Privacy and Data

- Blade does not enable usage telemetry in the current settings implementation.
- Local AI keeps inference on your configured local or LAN provider.
- Local storage stores conversation artifacts under the project `.zblade/` directory.
- Hosted Zaguán models and server storage require sending the relevant prompt, context, and conversation data to Zaguán services.
- `.gitignored` files are excluded from AI context by default and can be enabled per project in **Settings → Context**.

## 10. Support

Zaguán Blade is still evolving. Report bugs and feature requests on the [GitHub issue tracker](https://github.com/ZaguanLabs/ZaguanBlade/issues).
