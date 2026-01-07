# zblade Future Events - Roadmap

This document outlines planned events for future zblade features. These events are **not yet implemented** but are designed into the event contract to ensure architectural consistency as features are added.

---

## Phase 1: Core IDE Features (Next 3-6 months)

### Code Intelligence & Navigation

**Purpose:** LSP integration for code completion, navigation, and diagnostics

#### `symbol-found`
**Payload:** `{ file_path: string, line: number, column: number, symbol_name: string, symbol_type: string }`  
**Use Case:** Jump to definition, find references results

#### `code-completion-available`
**Payload:** `{ file_path: string, line: number, column: number, suggestions: CompletionItem[] }`  
**Use Case:** Show autocomplete suggestions from LSP

#### `diagnostics-updated`
**Payload:** `{ file_path: string, diagnostics: Diagnostic[] }`  
**Use Case:** Display errors, warnings, hints in editor

#### `hover-info-available`
**Payload:** `{ file_path: string, line: number, column: number, content: string, language: string }`  
**Use Case:** Show type info and documentation on hover

#### `references-found`
**Payload:** `{ symbol_name: string, references: Reference[] }`  
**Use Case:** Find all references to a symbol

#### `rename-preview`
**Payload:** `{ old_name: string, new_name: string, changes: FileChange[] }`  
**Use Case:** Preview symbol rename across files before applying

#### `code-action-available`
**Payload:** `{ file_path: string, line: number, actions: CodeAction[] }`  
**Use Case:** Quick fixes, refactorings (e.g., "Extract method", "Add import")

---

### Git Integration

**Purpose:** Real-time Git status and operations

#### `git-status-changed`
**Payload:** `{ modified: string[], staged: string[], untracked: string[], deleted: string[] }`  
**Use Case:** Update file explorer with Git status indicators

#### `git-branch-changed`
**Payload:** `{ previous_branch: string, current_branch: string }`  
**Use Case:** Update UI when switching branches

#### `git-commit-created`
**Payload:** `{ commit_hash: string, message: string, author: string }`  
**Use Case:** Show commit success notification

#### `git-conflicts-detected`
**Payload:** `{ files: string[], conflict_count: number }`  
**Use Case:** Alert user to merge conflicts

#### `git-diff-ready`
**Payload:** `{ file_path: string, diff: string, is_staged: boolean }`  
**Use Case:** Display diff view for file

#### `git-blame-loaded`
**Payload:** `{ file_path: string, blame_info: BlameInfo[] }`  
**Use Case:** Show Git blame annotations in editor

---

### Search & Find

**Purpose:** Fast workspace and file search

#### `search-results-ready`
**Payload:** `{ query: string, results: SearchResult[], total_matches: number, duration_ms: number }`  
**Use Case:** Display search results panel

#### `search-progress`
**Payload:** `{ query: string, files_searched: number, total_files: number, matches_found: number }`  
**Use Case:** Show search progress bar for large workspaces

#### `find-in-file-results`
**Payload:** `{ file_path: string, matches: Match[] }`  
**Use Case:** Highlight matches in current file

#### `replace-preview`
**Payload:** `{ query: string, replacement: string, affected_files: string[], total_replacements: number }`  
**Use Case:** Preview find/replace operation before applying

---

### Build & Run

**Purpose:** Build system integration and process management

#### `build-started`
**Payload:** `{ build_type: string, target: string }`  
**Use Case:** Show build in progress indicator

#### `build-progress`
**Payload:** `{ current_step: string, progress_percent: number, message: string }`  
**Use Case:** Update build progress bar

#### `build-completed`
**Payload:** `{ success: boolean, duration_ms: number, warnings: number, errors: number }`  
**Use Case:** Show build result notification

#### `build-error`
**Payload:** `{ file_path: string, line: number, column: number, message: string, severity: string }`  
**Use Case:** Navigate to build error in code

#### `test-run-started`
**Payload:** `{ test_suite: string, test_count: number }`  
**Use Case:** Show test run in progress

#### `test-run-completed`
**Payload:** `{ passed: number, failed: number, skipped: number, duration_ms: number }`  
**Use Case:** Show test results summary

#### `test-failed`
**Payload:** `{ test_name: string, file_path: string, line: number, error_message: string }`  
**Use Case:** Navigate to failing test

#### `process-started`
**Payload:** `{ process_id: string, command: string, working_directory: string }`  
**Use Case:** Track background processes

#### `process-output`
**Payload:** `{ process_id: string, output: string, is_error: boolean }`  
**Use Case:** Stream process output to terminal

#### `process-exited`
**Payload:** `{ process_id: string, exit_code: number, duration_ms: number }`  
**Use Case:** Show process completion status

---

## Phase 2: Advanced Features (6-12 months)

### Debugging

**Purpose:** Interactive debugging support

#### `debugger-attached`
**Payload:** `{ process_id: string, debugger_type: string }`  
**Use Case:** Enable debug UI controls

#### `debugger-detached`
**Payload:** `{ process_id: string }`  
**Use Case:** Disable debug UI controls

#### `breakpoint-hit`
**Payload:** `{ file_path: string, line: number, thread_id: string, stack_trace: StackFrame[] }`  
**Use Case:** Pause execution, show debug info

#### `breakpoint-added`
**Payload:** `{ file_path: string, line: number, condition?: string }`  
**Use Case:** Show breakpoint indicator in gutter

#### `breakpoint-removed`
**Payload:** `{ file_path: string, line: number }`  
**Use Case:** Remove breakpoint indicator

#### `debug-variable-updated`
**Payload:** `{ variable_name: string, value: string, type: string, scope: string }`  
**Use Case:** Update watch window

#### `debug-step-completed`
**Payload:** `{ step_type: string, file_path: string, line: number }`  
**Use Case:** Update execution pointer

---

### AI-Assisted Development

**Purpose:** Advanced AI features beyond chat

#### `ai-suggestion-available`
**Payload:** `{ file_path: string, line: number, suggestion: string, confidence: number }`  
**Use Case:** Inline code suggestions (like Copilot)

#### `ai-explanation-ready`
**Payload:** `{ code_snippet: string, explanation: string, language: string }`  
**Use Case:** Show code explanation in sidebar

#### `ai-refactor-proposed`
**Payload:** `{ refactor_type: string, description: string, changes: FileChange[] }`  
**Use Case:** Preview AI-suggested refactoring

#### `ai-test-generated`
**Payload:** `{ file_path: string, test_code: string, test_framework: string }`  
**Use Case:** Insert generated test code

#### `ai-documentation-generated`
**Payload:** `{ symbol_name: string, documentation: string, format: string }`  
**Use Case:** Insert generated docs/comments

#### `ai-code-review-ready`
**Payload:** `{ file_path: string, issues: CodeReviewIssue[], suggestions: string[] }`  
**Use Case:** Show AI code review feedback

#### `ai-context-indexed`
**Payload:** `{ files_indexed: number, symbols_indexed: number, duration_ms: number }`  
**Use Case:** Enable AI features that need codebase context

---

### Performance & Monitoring

**Purpose:** IDE health and stability

#### `memory-usage-warning`
**Payload:** `{ current_mb: number, limit_mb: number, percentage: number }`  
**Use Case:** Alert user to high memory usage

#### `cpu-usage-warning`
**Payload:** `{ current_percent: number, duration_seconds: number }`  
**Use Case:** Alert user to high CPU usage

#### `indexing-progress`
**Payload:** `{ files_indexed: number, total_files: number, current_file: string }`  
**Use Case:** Show indexing progress bar

#### `extension-loaded`
**Payload:** `{ extension_id: string, extension_name: string, version: string }`  
**Use Case:** Log extension loading

#### `extension-error`
**Payload:** `{ extension_id: string, error: string, stack_trace?: string }`  
**Use Case:** Show extension error notification

---

## Phase 3: Collaboration & Polish (12+ months)

### Collaboration & Remote

**Purpose:** Collaborative editing and remote development

#### `remote-cursor-moved`
**Payload:** `{ user_id: string, user_name: string, file_path: string, line: number, column: number }`  
**Use Case:** Show collaborator cursor positions

#### `remote-edit-received`
**Payload:** `{ user_id: string, file_path: string, edit: Edit }`  
**Use Case:** Apply collaborative edit

#### `remote-connection-status`
**Payload:** `{ status: string, remote_host: string, latency_ms?: number }`  
**Use Case:** Show remote dev environment status

#### `ssh-tunnel-status`
**Payload:** `{ status: string, local_port: number, remote_port: number, remote_host: string }`  
**Use Case:** Monitor SSH tunnel for remote dev

---

### UI & UX

**Purpose:** Responsive UI state management

#### `theme-changed`
**Payload:** `{ theme_id: string, theme_name: string, is_dark: boolean }`  
**Use Case:** Update UI colors

#### `layout-changed`
**Payload:** `{ layout_id: string, panels: PanelConfig[] }`  
**Use Case:** Persist layout preferences

#### `focus-changed`
**Payload:** `{ previous_panel: string, current_panel: string }`  
**Use Case:** Update keyboard shortcuts context

#### `command-palette-opened`
**Payload:** `{ trigger: string }`  
**Use Case:** Track command palette usage

#### `notification-shown`
**Payload:** `{ notification_id: string, type: string, message: string, duration_ms?: number }`  
**Use Case:** Manage notification queue

#### `modal-opened`
**Payload:** `{ modal_id: string, modal_type: string }`  
**Use Case:** Track modal stack

#### `modal-closed`
**Payload:** `{ modal_id: string, result?: any }`  
**Use Case:** Handle modal results

---

### Workspace Intelligence

**Purpose:** Proactive project understanding and assistance

#### `project-type-detected`
**Payload:** `{ project_type: string, language: string, framework?: string, build_system?: string }`  
**Use Case:** Configure IDE for project type

#### `dependencies-updated`
**Payload:** `{ manifest_file: string, added: string[], removed: string[], updated: string[] }`  
**Use Case:** Trigger dependency re-indexing

#### `dependency-vulnerability-found`
**Payload:** `{ package_name: string, version: string, vulnerability: VulnerabilityInfo }`  
**Use Case:** Alert user to security issues

#### `code-smell-detected`
**Payload:** `{ file_path: string, line: number, smell_type: string, severity: string, suggestion: string }`  
**Use Case:** Show code quality warnings

#### `todo-comments-found`
**Payload:** `{ todos: TodoItem[], fixmes: TodoItem[], total_count: number }`  
**Use Case:** Show TODO list panel

#### `documentation-outdated`
**Payload:** `{ file_path: string, symbol_name: string, reason: string }`  
**Use Case:** Suggest documentation updates

---

## Implementation Guidelines

When implementing these events:

1. **Add to `events.rs`**: Define event name constant and payload struct
2. **Add to `events.ts`**: Define matching TypeScript interface
3. **Update `EVENTS.md`**: Move from this file to main events documentation
4. **Implement emitter**: Add event emission in relevant Rust code
5. **Implement listener**: Add event handling in relevant React components
6. **Test**: Verify event flow and payload correctness

## Type Definitions

Common types referenced in payloads:

```typescript
interface CompletionItem {
  label: string;
  kind: string;
  detail?: string;
  documentation?: string;
}

interface Diagnostic {
  line: number;
  column: number;
  severity: 'error' | 'warning' | 'info' | 'hint';
  message: string;
  code?: string;
}

interface Reference {
  file_path: string;
  line: number;
  column: number;
  context: string;
}

interface FileChange {
  file_path: string;
  old_text: string;
  new_text: string;
}

interface CodeAction {
  title: string;
  kind: string;
  edit?: FileChange[];
}

interface SearchResult {
  file_path: string;
  line: number;
  column: number;
  match_text: string;
  context: string;
}

interface Match {
  line: number;
  column: number;
  length: number;
}

interface StackFrame {
  function_name: string;
  file_path: string;
  line: number;
  column: number;
}

interface CodeReviewIssue {
  line: number;
  severity: string;
  message: string;
  suggestion?: string;
}

interface Edit {
  start_line: number;
  start_column: number;
  end_line: number;
  end_column: number;
  text: string;
}

interface PanelConfig {
  panel_id: string;
  visible: boolean;
  size?: number;
  position?: string;
}

interface VulnerabilityInfo {
  cve_id?: string;
  severity: string;
  description: string;
  fixed_version?: string;
}

interface TodoItem {
  file_path: string;
  line: number;
  text: string;
  author?: string;
}
```
