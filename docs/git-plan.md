# Git Integration Plan

## Goals
- Add visual Git cues in the left sidebar near the File Explorer, including a dedicated Git icon with a badge showing the count of changed (tracked and staged) files.
- Provide a separate Git panel (same left region as File Explorer) for status, message generation, commit, and push.

## Scope
- UI integration only; no breaking changes to the File Explorer.
- Git operations should be safe, explicit, and user-confirmed.
- Support common workflows: view status, stage/unstage, generate message, commit, push.

## Assumptions
- Local workspace is a Git repository.
- We can shell out to Git (preferred for correctness) or use a library if sandboxed.

---

## Phase 1: Git Status & Sidebar Badge

### Functional Requirements
- Show a Git icon in the left sidebar (same vertical icon rail as Explorer).
- Badge indicates number of files with changes (staged + unstaged). Optionally show split counts later.
- Clicking the Git icon opens the Git panel (separate from Explorer).

### Data Model
- `GitStatusSummary`
  - `changedCount: number`
  - `stagedCount: number`
  - `unstagedCount: number`
  - `untrackedCount: number`
  - `branch: string | null`
  - `ahead: number`
  - `behind: number`
  - `dirty: boolean`

### Implementation Notes
- Poll Git status on:
  - Workspace change events (file save, file create/delete).
  - Focus/visibility changes.
  - Manual refresh button in Git panel.
- Use debounced refresh (e.g., 500–1000ms) to avoid thrashing.

### Proposed Git Status Command
- `git status --porcelain=v2 --branch` for reliable parsing.
  - Parse for branch name, ahead/behind, and file entries.
  - Aggregate counts into summary.

---

## Phase 2: Git Panel (Left Sidebar Section)

### UI Layout
- Left sidebar panel tabs: Explorer | Git
- Git panel sections:
  1. **Repository**: branch, ahead/behind, last fetch time, refresh button.
  2. **Changes**: list of unstaged + staged files.
  3. **Staging Controls**: stage/unstage per file or all.
  4. **Commit**: message input + optional generation.
  5. **Push**: push button (with upstream info).

### Required Actions
- View diffs per file (inline or open file with gutter markers later).
- Stage/unstage (per file & all).
- Commit with message.
- Push current branch.

---

## Commit Message Generation

### Approach
- Default to a simple AI-assisted generator using recent diffs and file list.
- Provide a “Generate” button next to commit message input.

### Data for Generation
- Include:
  - File list and paths.
  - Diff hunks (bounded size limit).
- Exclude:
  - Large binary changes.
  - Files ignored by `.gitignore`.

### Safety
- Show generated message in input for review before committing.
- Provide a “regenerate” action.

---

## Git Operations Implementation

### Preferred Strategy: Shell Git
- Pros: correct parsing, handles edge cases, respects global config.
- Commands:
  - Status: `git status --porcelain=v2 --branch`
  - Stage: `git add <path>` / `git add -A`
  - Unstage: `git restore --staged <path>`
  - Commit: `git commit -m "..."`
  - Push: `git push` (optionally `--set-upstream` on first push)
  - Diff: `git diff` / `git diff --staged`

### Alternative: Library
- Use `isomorphic-git` if sandboxed/no shell access.
- Requires custom fs bindings and potentially limited performance.

---

## UX Details
- Badge count = total changed files (staged + unstaged + untracked) by default.
- Optional toggle to show separate counts.
- Empty state:
  - “Working tree clean” message.
- Errors:
  - Display in-panel, non-blocking toast.
- “Not a Git repo” state:
  - Offer “Initialize Git” (optional future phase).

---

## Engineering Plan (Tasks)

1. **Discover existing sidebar architecture**
   - Identify icon rail component and how panels are registered.
2. **Add Git status service**
   - Create module for running Git commands and parsing status.
   - Expose `getStatusSummary()` and `getFileStatus()`.
3. **Add Git icon + badge**
   - Integrate with sidebar icon rail.
   - Connect badge to status summary.
4. **Build Git panel**
   - Create left panel content component.
   - List changes + stage/unstage actions.
5. **Commit workflow**
   - Add commit message input + generate button.
   - Hook to `git commit`.
6. **Push workflow**
   - Add push button and upstream handling.
7. **Diff view (optional follow-up)**
   - Show diff preview in Git panel.

---

## Risks & Mitigations
- **Performance**: Large repos can make status slow.
  - Mitigate with debounce and caching.
- **Parsing edge cases**: Git status output varies.
  - Use porcelain v2 for stability.
- **Security/permissions**: Shell execution may be blocked.
  - Fallback to library or require user approval.

---

## Acceptance Criteria
- Git icon appears in sidebar with badge count reflecting status.
- Git panel lists staged/unstaged files and supports stage/unstage.
- Commit message can be generated and used to commit.
- Push command works for current branch.
- Clear messaging for no-repo and clean states.
