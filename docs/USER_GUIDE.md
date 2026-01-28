# Zaguán Blade User Guide

Welcome to **Zaguán Blade**, the AI-Native code editor designed to collaborate with you.

This guide will help you get set up and understand how to work alongside your new AI pair programmer.

---

## 1. The Concept

Zaguán Blade is not just a text editor with a chat sidebar. It is designed from the ground up to allow an AI agent to "live" inside your editor.

*   **It sees what you see:** The AI has access to your active files, cursor position, and project structure.
*   **It acts:** The AI can run terminal commands, edit files, and browse the web to find answers.
*   **It collaborates:** Instead of just pasting code chunks, the AI proposes changes directly in your file using "Diff Blocks" which you can review, accept, or reject.

---

## 2. Getting Started

### Prerequisites: The API Key

**Important:** Zaguán Blade is a commercial AI product. To unlock its intelligence, you need an active subscription.

1.  Go to **[ZaguanAI.com](https://zaguanai.com/pricing)** and subscribe.
2.  Navigate to your account dashboard to copy your **Zaguán API Key**.

*Without a valid API Key, Zaguán Blade functions as a standard, high-performance text editor.*

### Configuration

1.  Launch Zaguán Blade.
2.  Click the **Gear Icon** (Settings) in the bottom-left corner of the Activity Bar.
3.  Go to the **Account** tab.
4.  Paste your **API Key**.
5.  (Optional) Use the **Test ZLP Connection** button to verify connectivity.

### First-Time Project Setup

When you open a project for the first time, Zaguán Blade will prompt you to choose a **Storage Mode** for your conversation history:

*   **Local Storage** (Recommended): Conversations are stored in a `.zblade/` folder within your project. Your code never leaves your machine.
*   **Server Storage**: Conversations are stored on Zaguán servers for faster context retrieval and cross-device sync.

You can change this setting later in **Settings → Storage**.

---

## 3. The Interface

The interface is streamlined to focus on code and conversation.

*   **Title Bar**:
    *   **File Menu**: Access New File, Open Folder, Save, Save As, and Exit.
    *   **Window Controls**: Minimize, Maximize/Restore, and Close buttons.

*   **Activity Bar** (Left Edge):
    *   **Files**: Your project file explorer.
    *   **Git**: View changed files, stage/unstage, commit, and push.
    *   **History**: Browse file history.
    *   **Settings**: Configure editor preferences and your account.

*   **Center Stage (The Editor)**:
    *   A high-performance editor based on CodeMirror 6.
    *   Supports syntax highlighting for major languages.
    *   **Diff Blocks**: When the AI proposes code, changes appear inline with green (added) and red (removed) highlighting.

*   **Right Panel (The AI Assistant)**:
    *   **Chat**: Your main communication channel with the Agent.
    *   **Model Selector**: Choose which AI model to use for responses.
    *   **Command Center**: The input box where you type instructions. Use `@` to access special commands.

*   **Bottom Panel (Terminal)**:
    *   Integrated terminal for running build commands, git operations, or anything else.
    *   The AI can see and interact with this terminal when executing commands.

---

## 4. Keyboard Shortcuts

### Global Shortcuts

| Shortcut | Action |
|----------|--------|
| `F11` | Toggle fullscreen mode |
| `Ctrl+W` | Close current tab |
| `Ctrl+Tab` | Cycle to next tab |
| `Ctrl+Shift+Tab` | Cycle to previous tab |
| `Escape` | Close modals/popups |

### File Operations

| Shortcut | Action |
|----------|--------|
| `Ctrl+N` | New File |
| `Ctrl+O` | Open Folder |
| `Ctrl+S` | Save current file |
| `Ctrl+Shift+S` | Save As |
| `Alt+F4` | Exit application |

### Editor Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+S` | Save file |
| `Ctrl+Z` | Undo |
| `Ctrl+Shift+Z` | Redo |
| `Ctrl+F` | Find in file |
| `Ctrl+X` | Cut selection |
| `Ctrl+C` | Copy selection |
| `Ctrl+V` | Paste |
| `F2` | Rename symbol |
| `Ctrl+E` | Toggle Edit/View mode (Markdown files) |

### Terminal Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+C` | Copy selection |
| `Ctrl+Shift+V` | Paste |

### Chat Input

| Shortcut | Action |
|----------|--------|
| `Enter` | Send message |
| `Shift+Enter` | New line (without sending) |
| `Arrow Up/Down` | Navigate command suggestions |
| `Tab` or `Enter` | Select command from autocomplete |
| `Escape` | Close command autocomplete |

---

## 5. @ Commands

Type `@` in the chat input to access special commands:

| Command | Description |
|---------|-------------|
| `@web <url>` | Fetches content from a URL and uses it as context for the AI |
| `@research <topic>` | Performs deep research on a topic and displays results in a new tab |

---

## 6. Working with the AI

### Context is Key

The AI automatically knows about the file you are currently looking at. You don't need to copy-paste code into the chat.

*   **Ask questions**: "Explain this function", "Refactor this to be more performant", "Find the bug in this logic".
*   **Tasking**: "Create a new component for X", "Update the CSS to match this design", "Run the tests and fix the failure".

### Reviewing Changes

When the AI writes code, it doesn't just overwrite your work. It proposes **Edits**.

1.  The AI will indicate it is writing code.
2.  You will see **Green** (added) and **Red** (removed) lines appear directly in your editor.
3.  **Review**: Read the changes.
4.  **Accept/Reject**:
    *   Click `Accept` (Checkmark) to permanently apply the changes.
    *   Click `Reject` (X) to discard them.
    *   You can also "Accept All" or "Reject All" via the floating action bar if there are multiple changes.

---

## 7. Settings

Access settings via the **Gear Icon** in the Activity Bar.

### Account Tab

*   **API Key**: Your Zaguán subscription key for AI features.
*   **Manage Subscription**: Link to your account dashboard.

### Storage Tab

*   **Storage Mode**: Choose between Local or Server storage for conversations.
    *   **Local**: Conversations stored in `.zblade/` folder. Maximum privacy.
    *   **Server**: Conversations stored on Zaguán servers. Faster context retrieval.
*   **Sync Metadata** (Local mode only): Sync conversation titles and tags to server (no code).
*   **Enable Cache**: Cache recent context for faster access.
*   **Max Cache Size**: Configure cache size (10-500 MB).

### Context Tab (Per-Project)

*   **Max Context Tokens**: Control how much context is sent to the AI (2K-32K tokens). Higher values provide more context but increase latency.
*   **Enable Compression**: Use AI to intelligently compress context.
    *   **Remote**: Uses cloud model for compression (faster).
    *   **Local**: Uses local model for compression (private).
*   **Allow .gitignored Files**: Include files matched by `.gitignore` in AI context. Disabled by default for security.

---

## 8. Project Instructions

Zaguán Blade creates a `.zblade/` folder in your project with an `instructions.md` file. Edit this file to provide project-specific instructions to the AI:

```markdown
# Project Instructions

## Project Overview
<!-- Describe your project briefly -->

## Coding Guidelines
<!-- Add any specific coding conventions or patterns to follow -->

## Important Files
<!-- List key files the AI should be aware of -->
```

The AI reads this file to understand your project's conventions and requirements.

---

## 9. Editor Features

### Context Menu (Right-Click)

Right-click in the editor to access:

*   **Cut / Copy / Paste**: Standard clipboard operations.
*   **Undo / Redo**: Edit history navigation.
*   **Find**: Open search panel.
*   **Rename Symbol**: Rename the symbol under cursor.
*   **Show Call Graph**: Visualize function call relationships.

### File Explorer Context Menu

Right-click on files/folders in the explorer:

*   **New File / New Folder**: Create items in the selected directory.
*   **Rename**: Rename the selected item.
*   **Delete**: Delete the selected item.
*   **Cut / Copy / Paste**: Move or copy files.
*   **Open in Terminal**: Open terminal at the selected location.

### Markdown Support

*   Markdown files (`.md`) automatically enable line wrapping.
*   Use `Ctrl+E` to toggle between Edit and View modes.

---

## 10. Privacy & Data

*   **No Telemetry**: Zaguán Blade does not collect usage telemetry.
*   **Local Storage Mode**: When using local storage, your code and conversations never leave your machine.
*   **Server Storage Mode**: Conversations are encrypted on Zaguán servers.

---

## 11. Support & Feedback

This is an **Alpha Release**. We define "Alpha" as "Feature incomplete, but good enough to be useful."

You *will* encounter bugs. When you do:

*   **Report Bugs**: Please file an issue on our [GitHub Issue Tracker](https://github.com/ZaguanLabs/ZaguanBlade/issues).
*   **Feature Requests**: We'd love to hear what you want to see next.

Thank you for being part of the future of coding.
