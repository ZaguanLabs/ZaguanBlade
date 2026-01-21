# ZBasic File Change Architecture
> Inspiration drawn from `vscode/src/vs/workbench/services/workingCopy` and `workingCopyHistory`.

## Core Philosophy: "Disk is Truth"

The previous model of using in-memory buffers for AI operations proved fragile. The new architecture adheres to a strict principle: **When the AI makes a change, it must be persisted to disk immediately.**

This shifts the responsibility of "state management" (undo, redo, review, diffing) from the *File System Layer* to the *Application Logic Layer*.

## 1. The "Working Copy" Concept
Inspiration: VSCode's `IWorkingCopy` and `StoredFileWorkingCopy`.

In VSCode, a "Working Copy" represents a file model that can be "Dirty" (unsaved).
- **VSCode**: User types -> Model becomes Dirty -> User presses Save -> Written to Disk.
- **ZBlade AI**: AI Tool executes -> Written to Disk -> Application State Updated.

For ZBlade, the "Working Copy" for an AI session is effectively the **Filesystem** itself, augmented by a **History Service**.

## 2. The "Shadow History" (The "Different logic")
Inspiration: VSCode's `WorkingCopyHistoryService`.

Since files are auto-saved by the AI, we cannot rely on the "Dirty" flag to indicate "New changes to review". Instead, we must implement a **History Service** that tracks versions of files as they are modified by the AI.

### Architecture

```rust
struct HistoryEntry {
    id: String,           // Unique ID (e.g., hash or timestamp)
    file_path: PathBuf,   // The file being tracked
    timestamp: u64,       // When the change happened
    source: ChangeSource, // Enum: User, AI_Tool, Undo, etc.
    metadata: ToolMetadata, // { tool_name: "replace_file", prompt_id: "..." }
    snapshot_path: PathBuf // Path to the backup/copy of this version in .zblade/history
}
```

### Workflow

1.  **AI Tool Action**:
    *   The AI triggers `replace_file_content`.
    *   **Step 1 (Pre-Write)**: The `HistoryService` creates a *snapshot* of the *current* file state (before the edit) and saves it to `.zblade/history/...`.
    *   **Step 2 (Write)**: The tool performs an atomic write to the target file on disk.
    *   **Step 3 (Post-Write)**: The `HistoryService` records a new `HistoryEntry` linking the snapshot to the tool action.
    *   **Step 4 (UI Update)**: The Editor reloads the file (which is now changed) and the "Review" UI updates to show the diff between `snapshot` and `current`.

## 3. Separation of Concerns

### A. The File System (Storage)
*   **Role**: Correctness and Persistence.
*   **Behavior**: Always reflects the latest applied changes. No ambiguous in-memory states for AI-generated code.
*   **Safety**: Relies on OS-level atomic writes to prevent corruption.

### B. The Application Layer (Logic)
*   **Role**: Workflow and Review.
*   **Behavior**: Tracks "Sessions".
    *   A "Session" might consist of 5 AI file edits.
    *   Since all 5 are definitely on disk, the Application Layer maintains the *pointer* to where the session started.
    *   **Diff View**: Calculated as `Diff(Start_Of_Session_Snapshot, Current_Disk_File)`.
    *   **Undo**: Executed by applying the content of a `HistoryEntry` snapshot back to the disk.

## 4. Inspiration from VSCode Source
*   **`StoredFileWorkingCopy`**: Bridges the gap between the Model and Disk. We should implement a similar reliable file watcher that knows when to reload the UI.
*   **`WorkingCopyHistoryModel`**: Manages the list of entries. We need a similar robust index (JSON based or SQLite) to track the history of files without cluttering the user's view.
*   **Locking**: VSCode uses `TaskSequentializer` to prevent race conditions during saves. ZBlade must implement a file lock or queue system to ensure the AI doesn't try to read/write the same file simultaneously (e.g., rapid tool loops).

## 5. Implementation Plan

1.  **`HistoryService` struct**:
    *   Manages `.zblade/history` directory.
    *   Implements `create_snapshot(path)` -> returns `snapshot_id`.
    *   Implements `revert_to(snapshot_id)`.

2.  **ToolWrapper**:
    *   Wrap all file-modifying tools (`write_file`, `replace_file_content`) with a middleware.
    *   Middleware automatically calls `HistoryService::create_snapshot` before execution.

3.  **Frontend Integration**:
    *   The frontend no longer holds "pending" diffs in memory.
    *   It queries `zcoderd` for "History since last user interaction".
    *   It renders the Diff view based on the snapshots.

## 6. Why this is better
*   **Crash Proof**: If ZBlade crashes mid-generation, the file is safely on disk (or the previous version is safely in history).
*   **Simpler Mental Model**: "What you see is what is on disk".
*   **Scalable**: We can store infinite history (limited by disk space) without consuming RAM.
