# Zaguán Blade User Guide

Welcome to **Zaguán Blade**, the AI-Native code editor designed to collaborate with you.

This guide will help you get set up and understand how to work alongside your new AI pair programmer.

## 1. The concept

Zaguán Blade is not just a text editor with a chat sidebar. It is designed from the ground up to allow an AI agent to "live" inside your editor.

*   **It sees what you see:** The AI has access to your active files, cursor position, and project structure.
*   **It acts:** The AI can run terminal commands, edit files, and browse the web to find answers.
*   **It collaborates:** Instead of just pasting code chunks, the AI proposes changes directly in your file using "Diff Blocks" which you can review, accept, or reject.

## 2. Getting Started

### Prerequisites: The API Key

**Important:** Zaguán Blade is a commercial AI product. To unlock its intelligence, you need an active subscription.

1.  Go to **[ZaguanAI.com](https://zaguanai.com/pricing)** and subscribe.
2.  Navigate to your account dashboard to copy your **Zaguán API Key**.

*Without a valid API Key, Zaguán Blade functions as a standard, high-performance text editor.*

### Configuration

1.  Launch Zaguán Blade.
2.  Click the **Gear Icon** (Settings) in the bottom-left corner.
3.  Go to the **Account** tab.
4.  Paste your **API Key**.
5.  (Optional) Verify the `Status` turns green.

## 3. The Interface

The interface is streamlined to focus on code and conversation.

*   **Left Sidebar**:
    *   **Files**: Your project file explorer.
    *   **Search**: Fast file search.
    *   **Settings**: Configure editor preferences and your account.
*   **Center Stage (The Editor)**:
    *   A high-performance editor based on CodeMirror 6.
    *   Supports syntax highlighting for major languages.
    *   **Vertical Diff Blocks**: When the AI proposes code, it appears right here in context.
*   **Right Panel (The AI Assistant)**:
    *   **Chat**: Your main communication channel with the Agent.
    *   **Command Center**: The input box where you type instructions. Use `@` to quickly access specific tools (e.g., `@search`, `@web`).
*   **Bottom Panel (Terminal)**:
    *   Integrated terminal for running build commands, git operations, or anything else. The AI can also see and interact with this terminal if you allow it.

## 4. Working with the AI

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

### Web Research

Zaguán Blade is connected to the world.

*   Use `@search` to ask the AI to find libraries, documentation, or solutions online.
*   Use `@web` to have the AI read a specific URL (like a documentation page) to understand how to use a new tool.

## 5. Support & Feedback

This is an **Alpha Release**. We define "Alpha" as "Feature incomplete, but good enough to be useful."

You *will* encounter bugs. When you do:

*   **Report Bugs**: Please file an issue on our [GitHub Issue Tracker](https://github.com/ZaguanLabs/ZaguanBlade/issues).
*   **Feature Requests**: We'd love to hear what you want to see next.

Thank you for being part of the future of coding.
