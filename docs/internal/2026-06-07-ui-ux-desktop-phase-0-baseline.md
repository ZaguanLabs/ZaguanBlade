# Desktop UI/UX Phase 0 Baseline

Date: 2026-06-07

Related plan: `docs/internal/2026-06-07-ui-ux-desktop-implementation.md`

## Phase goal

Phase 0 exists to make the desktop UI/UX migration recoverable before any visual shell work begins. This phase should not change runtime behavior, layout, component semantics, project state, editor buffers, terminal state, chat state, or backend behavior.

## Current safety note

The README now includes a temporary notice that the main tree may be unstable while the desktop UI/UX migration is in progress.

## Current worktree state

Before Phase 0 changes, the repository already had unrelated modified and untracked files. Treat those as user or in-progress work and do not revert them as part of the UI/UX migration.

Known pre-existing changed frontend files included:

- `src/components/CompactModelSelector.tsx`
- `src/components/Layout.tsx`
- `src/components/ModelSelector.tsx`
- `src/components/Terminal.tsx`
- `src/components/TerminalPane.tsx`
- `src/hooks/useCommandExecution.test.ts`
- `src/hooks/useCommandExecution.ts`
- `src/utils/dropdownScroll.ts`
- `src/utils/indexHealthStatus.test.ts`
- `src/utils/indexHealthStatus.ts`

Known pre-existing changed Rust/Tauri files included several files under `src-tauri/src`.

Phase 0 changes added or modified only:

- `README.md`
- `docs/internal/2026-06-07-ui-ux-desktop-phase-0-baseline.md`

## Confirmed current defaults

### Chat panel path

`src/contexts/ChatPanelFlagContext.tsx` resolves `chatPanelV3` as:

1. `localStorage["chat.panel.v3"]` if set.
2. Remote settings `configuration["chat.panel.v3"]` or `feature_flags["chat.panel.v3"]` if set.
3. `true` by default.

So Chat Panel V3 is the default path unless locally or remotely disabled.

### Existing debug and isolation flags

Current UI isolation/debug flags already present in `src/components/Layout.tsx` and `src/utils/debugFlags.ts`:

- `disableTerminal`
- `disableEditor`
- `disableChat`
- `disableChatHook`
- `disableGitStatus`
- `disableUncommittedChanges`
- `disableLayoutEvents`
- `disableProjectState`
- `disableWarmup`
- `disableTabManager`
- `disableActivityBar`
- `disableSidebarOverlay`
- `disableEditorChrome`
- `disableChatChrome`
- `disableEditorWidthObserver`
- `debugPerf`

These are useful for isolating surfaces during the migration and should not be removed.

### Existing performance helper

`src/utils/debugPerf.ts` already provides `recordDebugPerf()` and `startDebugPerfReporter()` behind `debugPerf`. No new performance helper is needed for Phase 0.

## Baseline files to protect

Shell and layout:

- `src/components/Layout.tsx`
- `src/components/AppBar.tsx`
- `src/components/TitleBar.tsx`
- `src/components/ui/Surface.tsx`
- `src/components/ui/ListRow.tsx`
- `src/components/ui/IconButton.tsx`
- `src/components/ui/Modal.tsx`
- `src/styles/theme.css`
- `src/index.css`

Primary panes:

- `src/components/EditorPanel.tsx`
- `src/components/ExplorerPanel.tsx`
- `src/components/FileExplorer.tsx`
- `src/components/GitPanel.tsx`
- `src/components/TerminalPane.tsx`
- `src/components/Terminal.tsx`
- `src/chat/rendering/ChatPanel.tsx`
- `src/chat/rendering/ChatViewport.tsx`
- `src/chat/composer/Composer.tsx`

Behavior and persistence hooks:

- `src/hooks/useResizeHandlers.ts`
- `src/hooks/useProjectState.ts`
- `src/hooks/useTabManager.ts`
- `src/hooks/useChat.ts`
- `src/hooks/useChatV2.ts`
- `src/hooks/useCommandExecution.ts`
- `src/hooks/useUncommittedChanges.ts`

## Baseline automated checks

Run from repo root:

```bash
bun test src/**/*.test.ts
bun run build
```

Record results in this document before Phase 1 begins.

Result on 2026-06-07:

- `bun test src/**/*.test.ts`: passed, 121 tests across 14 files.
- `bun run build`: passed, TypeScript and Vite production build completed.
- Build note: Vite reported plugin timing hotspots, with `vite:terser` taking the largest share. This is informational for Phase 0 and not a UI migration blocker.

## Baseline manual screenshot checklist

Screenshots still need to be captured from a running app session before Phase 1 begins:

- Fresh launch with no file selected.
- Workspace with one file tab.
- Workspace with multiple tabs.
- Sidebar explorer open.
- Sidebar git open.
- Sidebar history open.
- Terminal closed.
- Terminal open at saved height.
- Chat empty.
- Chat with a long conversation.
- Pending command approval.
- Pending uncommitted changes.
- Settings modal.
- Narrow app width near minimum.
- Maximized window.

Deferral recorded on 2026-06-07:

- Deferred until a real Tauri app session is available.
- Reason: this environment does not have Playwright installed, and a plain Vite/browser capture would not faithfully validate Tauri window chrome, native resize behavior, terminal integration, backend-backed panes, or app-state restoration.
- Requirement before shell-changing PRs: capture these screenshots from the actual desktop app, not from a browser-only fallback.

## Baseline manual workflow checklist

Run manually before Phase 1 begins:

- Open file from explorer.
- Switch between tabs.
- Reorder tab by drag.
- Close tab.
- Open sidebar, switch sidebar view, close sidebar.
- Open terminal, resize terminal, close/reopen app and confirm height restore.
- Resize chat panel, close/reopen app and confirm width restore.
- Send chat message.
- Stop chat generation.
- Approve and reject a command.
- Accept and reject AI file changes.
- Open settings.
- Close app with clean state.
- Close app with dirty editor state and verify save/discard/cancel.

Deferral recorded on 2026-06-07:

- Deferred until a real Tauri app session is available.
- Reason: the checklist covers native window close behavior, dirty-save prompts, terminal restoration, chat command approval, and backend-driven file changes. Those are not trustworthy in a browser-only session.
- Requirement before shell-changing PRs: complete this workflow in the actual desktop app and record any issues here.

## Phase 0 completion criteria

Phase 0 is complete only when:

- [x] README warning exists.
- [x] Automated check results are recorded.
- [x] Manual screenshot checklist is either completed or explicitly deferred with reason.
- [x] Manual workflow checklist is either completed or explicitly deferred with reason.
- [x] No runtime UI migration changes have been made.

## Current Phase 0 status

Phase 0 documentation and automated baseline portion is complete.

Remaining before shell-changing PRs:

- Capture the baseline screenshots from a running app session.
- Run the baseline manual workflow checklist from a running app session.
- Record any visual or workflow issues discovered during that session.

Phase 1 token cleanup may begin before those manual items because it should not move panes, alter runtime behavior, or change Tauri shell mechanics. Phase 2 shell migration must not begin until the manual app-session baseline is complete.

## Rollback

Rollback for Phase 0 is simple:

- Revert the README notice.
- Delete this baseline document.
