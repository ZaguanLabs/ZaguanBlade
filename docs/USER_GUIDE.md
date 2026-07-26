# Zaguán Blade User Guide

This guide describes Zaguán Blade 0.9.2. Blade is an open source desktop code editor for AI-assisted engineering work. It can run with local AI providers, or connect to the hosted Zaguán Coder Daemon for the full subscription-backed workflow.

## 1. What Blade Does

Blade is built around an inspect, change, and validate loop:

- It gives the assistant controlled access to editor state, open files, cursor position, selected paths, project structure, symbol relationships, terminal output, Git state, and uncommitted changes.
- It can investigate a workspace, propose or apply file edits, run approved commands, and help validate the result.
- AI edits are written to disk with history snapshots and remain reviewable until you accept or reject them.
- Code and Plan modes let you choose between implementation and read-only investigation.

Blade is local-first, but “local” has two separate meanings:

- **Local AI** controls where model inference runs.
- **Local Storage** controls where conversation history is stored.

You can use either setting independently.

## 2. Getting Started

### Open a Project

Choose **File → Open Project**, or use the open-project action on the welcome screen. Blade restores project state such as open files and prepares its local code-intelligence index in the background.

The status bar reports whether code intelligence is checking, indexing, finalizing, ready, partial, or unavailable. You can begin working while indexing continues, although symbol-aware results improve once it is ready.

### Set Up Local AI

You can use Blade without a Zaguán subscription by enabling a local provider:

1. Open **Settings → Local AI**.
2. Enable one or more providers:
   - **Ollama (Local)** at `http://localhost:11434`
   - **Ollama Cloud** with an Ollama API key
   - **OpenAI-compatible Server** for llama.cpp, LocalAI, vLLM, and similar servers
3. Use **Test Connection**.
4. Use **Refresh Models**.
5. In the **Models** list, choose which discovered models should appear in the chat model picker.
6. Save the settings and select a model in chat.

The Models list also shows whether each model uses Blade’s built-in system prompt or a custom prompt saved for that model. See [OPENAI_COMPAT_SETUP.md](OPENAI_COMPAT_SETUP.md) for provider setup, prompt locations, filename matching, and troubleshooting.

Local model quality and tool-calling reliability depend on the model and server. Local providers do not have built-in web fetch or deep research, and image attachments are unavailable when Blade only has local models.

### Connect a Zaguán Account

A Zaguán subscription connects Blade to the hosted Zaguán Coder Daemon:

1. Subscribe at [ZaguanAI.com](https://zaguanai.com/pricing).
2. Open **Settings → Account**.
3. Select **Sign in**.
4. Complete the browser flow. If needed, use the displayed device code and verification-page link.
5. Return to Blade after the account is approved.

The Account page shows the connected account and plan, and links to subscription, device, usage, and credit management. You can reconnect or sign out from the same page. Manual API-key entry remains available for existing keys and local development.

Hosted models appear in the model picker after authentication. Hosted workflows can use stronger models, managed orchestration, richer context handling, and web-backed research where available.

### Choose Conversation Storage

When you open a project for the first time, Blade asks where to store conversation history:

- **Local Storage** stores conversations and local artifacts under `.zblade/` in the project.
- **Server Storage** stores conversations on Zaguán servers for faster retrieval and cross-device sync.

Blade also keeps project settings, indexes, cache files, and history artifacts under `.zblade/`. It ensures `.zblade` is ignored by the workspace Git repository.

## 3. Interface Overview

### App Bar and Tabs

The app bar contains the File menu, editor tabs, project title, fullscreen handling, and window controls.

Tabs show unsaved files, AI-edited files, pending review state, unread AI edits, and files deleted on disk. You can drag tabs to reorder them. The tab context menu can copy the filename or full path, move a tab to the beginning or end, close other tabs, or close all tabs.

When quitting with unsaved editor changes, Blade asks whether to save them.

### Activity Bar and Sidebar

- **Explorer**: browse and manage files. The lower Outline section shows symbols for the active file.
- **Git**: inspect changes, view diffs, stage or unstage files, commit, push or publish a branch, and browse the Git graph.
- **File History**: view snapshots for the active file and revert to an earlier version.
- **Settings**: configure account, local AI, storage, context, remote control, appearance, language, and app details.

Clicking the active sidebar icon closes that sidebar. Clicking outside an open sidebar also closes it.

### Editor

The editor is based on CodeMirror 6 and supports syntax highlighting for Rust, TypeScript, JavaScript, Python, Go, C/C++, HTML, CSS, JSON, YAML, PHP, Markdown, and related formats.

Depending on language support, the editor can show diagnostics and symbol information on hover. The active file’s Outline provides quick navigation to classes, functions, methods, headings, and other indexed structures.

Markdown files can switch between edit and rendered view modes with `Ctrl+E`.

PDF files open in a built-in viewer with page navigation, zoom, and fit-to-width controls.

### Chat and History

The right panel has two tabs:

- **Chat** contains the current conversation, model and mode controls, active task status, tool progress, queued requests, and the composer.
- **History** groups saved conversations by date. Select a conversation to resume it, or use **New Conversation** to start fresh.

### Terminal

Blade includes integrated terminal tabs. You can create and close terminals, copy or paste, search output, clear the terminal, and send selected terminal content to chat.

AI-triggered commands appear in the terminal and require approval unless project YOLO mode is enabled. Approved long-running commands may continue as background sessions while the assistant checks output, writes to standard input, or stops the process.

## 4. Working With Files and Code

### Explorer Actions

The Explorer supports:

- New files and folders
- Rename and delete
- Cut, copy, paste, and duplicate
- Drag-and-drop moves
- Copy full or workspace-relative paths
- Reveal in the system file manager
- Open a folder in the integrated terminal
- Manual refresh

Right-click a file or folder to see the actions available for that item.

### Outline and Local Symbol Graph

The Outline below the Explorer shows the active file’s parsed structure. Select an entry to navigate to it.

To investigate a symbol visually:

1. Put the cursor on an indexed symbol.
2. Right-click and choose **Show Symbol Graph**.
3. Filter the graph by incoming or outgoing relationships, relationship type, and minimum confidence.
4. Select a related symbol to open it, or expand it to continue exploring its neighborhood.

The graph can show calls, dependencies, type relationships, handlers, and structural relationships. Results are derived from the local index; supported relationships and confidence vary by language.

### Code-Intelligence Index

Blade maintains a local, workspace-wide symbol and semantic index. The assistant can use it to:

- Find definitions, references, callers, implementations, and related symbols
- Read file outlines and exact symbol ranges
- Trace relationships across multiple steps
- Estimate the impact and likely tests for a proposed edit
- Map modules and architecture
- Find semantic anchors such as routes, configuration keys, environment access, translation keys, CSS selectors, and design references

Coverage includes full or partial indexing for Rust, TypeScript/JavaScript, Python, Go, C/C++, Java, C#, Kotlin, Ruby, PHP, Vue and Svelte scripts, shell, SQL, Dockerfiles, Make/CMake, Markdown, and common web and configuration formats. Coverage is not identical for every language.

The status bar provides detailed index progress and health. A partial index can still be useful, but empty results are less conclusive until the relevant files are indexed.

## 5. Working With AI

### Code and Plan Modes

Use **Plan** mode when you want read-only investigation and a concrete implementation plan. Plan mode tells the assistant not to edit files or run commands. After a plan is produced, use **Implement** to send it into a Code-mode request.

Use **Code** mode when you want the assistant to implement changes, use tools, run approved commands, and validate its work.

Useful request patterns:

- “Explain the active file.”
- “Find where this symbol is used.”
- “Show the impact of changing this public function.”
- “Plan the safest way to refactor this component.”
- “Run the relevant tests and fix the failure.”
- “Use `@src/path/to/file.ts` as context.”
- “Use the deployment skill for this task.”

### Add Explicit Context

Type `@` in the composer to open suggestions:

- Select a workspace file or folder to attach it as an explicit reference.
- Use `@web` with a URL to fetch web content in supported hosted workflows.
- Use `@research` to start hosted deep research and open the completed result in a new editor tab.

The assistant also receives relevant live state such as the active file, cursor or selection, open files, project instructions, and Git changes. It can request additional context through the local index and file tools.

### Composer and Queue

Press `Enter` to send and `Shift+Enter` for a newline. While the assistant is responding, additional requests are queued instead of being lost. Each queued request can be edited or deleted before it runs.

Use the stop control to cancel the active response. The chat shows task progress, reasoning summaries where provided, tool activity, result counts, and command approval state. A floating approval indicator helps you return to a pending command when it is outside the visible chat area.

The composer has its own undo and redo history with `Ctrl+Z` and `Ctrl+Shift+Z`. At the start or end of the input, use the Up and Down arrow keys to revisit previously sent prompts.

### Conversation Actions

Right-click a message to copy it. Right-click one of your own editable messages and choose **Edit Message** to revise the request; the conversation continues again from that point.

Use the History tab to resume older local or server-stored conversations.

### Cloud Error Recovery

If a hosted request fails because credits or account access need attention, chat provides direct account or credit links and a **Use Local AI** action.

## 6. Repository Instructions and Agent Skills

### `AGENTS.md`

Blade automatically loads workspace instructions from `AGENTS.md`:

- The root `AGENTS.md` applies across the workspace.
- Nested `AGENTS.md` files apply to work under their directory.
- Instructions can include local Markdown files such as `@workflow.md` or `@docs/workflow.md`.

Use these files for repository-specific commands, conventions, safety rules, validation expectations, and documentation requirements.

### Skills

Skills are reusable workflows that the assistant can discover and load only when relevant. Blade supports them for hosted models and direct local providers.

Blade discovers `SKILL.md` files under:

- `.agents/skills/` in the current workspace
- `~/.agents/skills/` for user-wide skills
- Blade’s legacy global skills directory for existing installations

A minimal workspace skill looks like this:

```markdown
---
name: release-check
description: Validate a release candidate and prepare release notes.
---

Follow the repository release checklist.
Read `references/release-policy.md` before changing version files.
```

Put it at `.agents/skills/release-check/SKILL.md`. Referenced files and scripts can live beside it. Keep the catalog description clear enough for the assistant to decide when the skill applies, or name the skill explicitly in your request.

Blade advertises the available skill catalog without loading every instruction file into every prompt. The assistant can search the catalog, load the selected skill in bounded chunks, and then read only the referenced resources it needs.

## 7. Reviewing AI Actions

### File Changes

AI file changes are written to disk with a history snapshot and tracked as pending Blade changes:

- **Accept** keeps the current file contents and clears its pending review state.
- **Reject** restores the snapshot captured before the AI change.
- **Accept All** and **Reject All** operate on the current pending change set.
- File tabs and editor controls show pending and unread AI edits.
- Repeated AI edits preserve the combined review diff.

Tool cards can also expose an **Undo** action for supported file changes. The Git panel remains the source of truth for repository staging and commits.

### Command and Tool Approvals

When the assistant requests a command, Blade shows the command and lets you run or skip it. Some other sensitive tool actions can also show an approval card.

**YOLO mode** auto-approves `run_command` requests for the current project. It does not silently approve every kind of tool action, and the model is not told that YOLO mode is enabled.

Remote control approvals follow the same run-or-reject model.

## 8. Git Workflow

The Git sidebar shows the current branch, ahead/behind state, staged files, unstaged files, untracked files, and conflicts.

You can:

- Expand a file to inspect its diff
- Stage or unstage one file
- Stage all or unstage all
- Enter a commit message and commit
- Generate a commit message with the selected AI model
- Push commits or publish a branch that does not yet have an upstream
- Expand the Git graph to browse commits
- Copy a commit hash or open a compatible commit on GitHub

Blade warns about detached HEAD and blocks commits with unresolved conflicts. Committing with unstaged changes stages them as part of the commit. Pushing accepts the current pending Blade AI-change set because those files are being published to Git.

## 9. Screenshots, Images, and Documents

### Attach an Image

Use the **Add** button in the composer to:

- Capture a window
- Select a window and crop a region
- Upload an image from disk
- Paste an image directly into the composer

After capture, Blade opens an annotation editor. It includes select, arrow, text, outlined or filled shapes, pencil, color and size controls, undo, redo, delete, and clear.

Image attachments require a supported hosted model. They are disabled when only local models are available and for known unsupported models such as GLM.

### Linux and X11 Capture Notes

On X11 desktops, capture is limited to windows visible on the current workspace. Covered windows can capture as black unless a compositor such as `picom` is running. Bring the target window forward before capture. Wayland, macOS, and composited desktops generally avoid these X11 limitations.

### View PDFs

Open a PDF from the Explorer to use the built-in document viewer. Its toolbar provides previous/next page, zoom in/out, and fit-to-width controls. Scrolling updates the current-page indicator.

## 10. Settings Reference

### Configuration

- Built-in dark and light themes
- Editor and chat text size
- English or Spanish (Spain) interface language
- Project-scoped YOLO mode

You can also use `Ctrl+mouse wheel` over the editor or chat to adjust and save that surface’s text size.

### Account

- Browser-based Zaguán sign-in and reconnect
- Connected account and plan
- Subscription, device, usage, and credit links
- Sign out
- Manual API-key entry

### Local AI

- Ollama local URL
- Ollama Cloud API key
- OpenAI-compatible server URL
- Connection testing and model refresh
- Per-model visibility in the chat model picker
- Built-in versus custom-prompt status

### Storage

- Local or server conversation storage
- Metadata sync for local storage
- Context cache toggle and cache size

### Context

This section is available when a workspace is open:

- Maximum context tokens from 2K to 32K
- Context compression toggle
- Remote or local compression model
- Whether `.gitignored` files may be included in AI context
- **Warmup Context Prefetch**

Warmup Context Prefetch is enabled by default. Supported providers receive the active editor file and applicable repository instructions during connection warmup, which can reduce first-response latency and repeated prompt-cache cost. Disable it if you prefer the assistant to read those resources only on demand.

Allowing `.gitignored` files can expose secrets, generated output, or large artifacts to the assistant. Leave it disabled unless the task genuinely requires those files.

### Remote

Remote control pairs Blade with your own Telegram bot. Once paired, you can send terminal commands, approve or reject AI command execution, and view command output and exit codes from your phone. Blade must remain running on your computer.

### About

Version, runtime, engine, mode, website, GitHub, and support links.

## 11. Keyboard Shortcuts

### App and Tabs

| Shortcut | Action |
|----------|--------|
| `F11` | Toggle fullscreen |
| `Ctrl+W` | Close current tab |
| `Ctrl+Tab` | Cycle to next tab |
| `Ctrl+Shift+Tab` | Cycle to previous tab |
| `Escape` | Close the active modal, picker, or menu |

### Files and Editor

| Shortcut | Action |
|----------|--------|
| `Ctrl+N` | New file in the Explorer |
| `Ctrl+Shift+N` | New folder in the Explorer |
| `Ctrl+S` | Save the active file |
| `Ctrl+Z` | Undo |
| `Ctrl+Shift+Z` | Redo |
| `Ctrl+F` | Find in file |
| `Ctrl+X` / `Ctrl+C` / `Ctrl+V` | Cut, copy, and paste |
| `Ctrl+E` | Toggle Markdown edit/view mode |

### Chat Composer

| Shortcut | Action |
|----------|--------|
| `Enter` | Send message |
| `Shift+Enter` | Insert newline |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo or redo composer edits |
| `@` | Open command and workspace-path suggestions |
| `Tab` or `Enter` | Accept the active suggestion |
| `Escape` | Close suggestions |
| `Arrow Up` / `Arrow Down` | Navigate message history at the start/end of the composer |

### Screenshot Annotation

| Shortcut | Action |
|----------|--------|
| `Ctrl+Z` | Undo annotation |
| `Ctrl+Y` or `Ctrl+Shift+Z` | Redo annotation |
| `Delete` or `Backspace` | Delete selected annotation |
| `Ctrl+Enter` | Finish and attach |
| `Escape` | Cancel |

### Terminal

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+C` | Copy terminal selection |
| `Ctrl+Shift+V` | Paste into terminal |
| `Ctrl+F` | Find in terminal |

## 12. Privacy and Data

- Blade does not enable usage telemetry in the current settings implementation.
- Local AI keeps inference on your configured local or LAN provider.
- Local Storage keeps conversation artifacts under the project `.zblade/` directory.
- Hosted Zaguán models and Server Storage require sending the relevant prompt, context, and conversation data to Zaguán services.
- `.gitignored` files are excluded from AI context by default and can be enabled per project in **Settings → Context**.
- Remote control uses a Telegram bot token you provide and requires Blade to be running.
- Keyless OpenAI-compatible servers should be bound to localhost or a trusted private network, not exposed directly to the public internet.

## 13. Support

Zaguán Blade is still evolving. Report bugs and feature requests on the [GitHub issue tracker](https://github.com/ZaguanLabs/ZaguanBlade/issues).
