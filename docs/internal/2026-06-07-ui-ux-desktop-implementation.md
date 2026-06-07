# Desktop UI/UX Implementation Plan

Date: 2026-06-07

Related research: `docs/internal/2026-06-07-ui-ux-best-practices-2026.md`

## Purpose

This document converts the UI/UX research into a conservative implementation plan. The goal is to make Zaguan Blade feel more like a stable desktop application without destabilizing the editor, terminal, chat, file explorer, git workflow, project state, or Tauri shell.

The work must be efficient, correct, reversible, and biased toward removing visual and interaction risk before adding new surface area.

## Operating principles

### 1. Remove more than we add

Default action order:

1. Remove decoration from persistent chrome.
2. Replace broad abstractions with smaller desktop primitives.
3. Rename misleading tokens and variants.
4. Add new components only when reuse or correctness requires them.

Examples:

- Remove persistent pane shadows before creating new pane effects.
- Remove nested cards before adding new layout primitives.
- Remove large empty-state marketing layout before adding any new first-run experience.
- Remove ad hoc click `div`s before adding richer rail behavior.

### 2. One risk class per PR

Do not mix visual shell changes, accessibility semantics, command registry, chat virtualization, and AI workflow changes in one PR.

Each PR should fit one risk class:

- Token/CSS-only visual reduction.
- Semantics-only component replacement.
- Layout-only pane migration.
- Command wiring.
- Performance instrumentation.
- Performance behavior change.
- AI workflow state presentation.

### 3. Preserve runtime behavior first

The first pass should not change how data flows through:

- `useChat` / `useChatV2`
- `useTabManager`
- `useProjectState`
- `useResizeHandlers`
- `useUncommittedChanges`
- CodeMirror document state
- xterm terminal state
- Tauri window controls

If a change requires touching one of these, it should be isolated and tested separately.

### 4. Reversible by construction

Every phase should have a rollback path that is one of:

- Revert one PR.
- Disable a feature flag.
- Switch one component import back to the old component.
- Restore old CSS token values.

Avoid migrations that require coordinated frontend/backend state changes unless the phase explicitly calls for it.

### 5. Visual correctness is a testable property

For shell work, a PR is incomplete without screenshots or visual inspection notes for:

- Default app with no sidebar.
- Sidebar open.
- Chat active with messages.
- Terminal open.
- Settings modal.
- Empty editor state.
- Narrow window.
- Fullscreen/maximized window.

### 6. Performance is part of correctness

Do not accept visual changes that make common interactions slower.

Core interaction budget:

- Sidebar toggle: no visible lag.
- Tab switch: no visible lag.
- Composer typing: no frame drops.
- Chat streaming: no scroll jump or long freeze.
- Context menu open: immediate.
- Terminal resize: stable and responsive.

## Safety model

### Branch strategy

Use a dedicated branch for the full migration, but keep each phase as small PRs:

```text
desktop-ui-phase-0-baseline
desktop-ui-phase-1-tokens
desktop-ui-phase-2-shell
desktop-ui-phase-3-semantics
desktop-ui-phase-4-command-model
desktop-ui-phase-5-performance
desktop-ui-phase-6-ai-workflow
```

Each PR should include:

- Scope.
- Screenshots or visual notes.
- Manual test checklist result.
- Automated test result.
- Explicit rollback path.

### Feature flags

Use flags only for behavior or layout changes that may need runtime rollback. Do not flag pure token cleanup.

Recommended flags:

- `desktopShellV1`: switches persistent panes from floating card shell to integrated pane shell.
- `semanticActivityRail`: uses button-based activity rail.
- `semanticAppTabs`: uses semantic/keyboard-navigable tabs.
- `commandRegistryV1`: routes shortcuts/menus through a command registry.
- `chatViewportVirtualizationV1`: enables Chat V3 virtualization or containment strategy.
- `compactEmptyStatesV1`: uses desktop-style empty states.

Avoid flag explosion. Remove flags after two stable releases or after the old path is deleted.

### Rollback policy

Rollback should be immediate if any of these occur:

- Editor loses unsaved content.
- Terminal sessions fail to restore.
- Project state restore breaks.
- Window close/save prompt regresses.
- Chat send/stop/approval flow breaks.
- Accept/reject uncommitted changes breaks.
- App fails to launch on any supported platform.

Visual imperfections should normally be fixed forward unless they block workflows or make text unreadable.

## Baseline inventory

Before implementation, capture the current state.

### Files to inventory

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

State and behavior hooks:

- `src/hooks/useResizeHandlers.ts`
- `src/hooks/useProjectState.ts`
- `src/hooks/useTabManager.ts`
- `src/hooks/useChat.ts`
- `src/hooks/useChatV2.ts`
- `src/hooks/useCommandExecution.ts`
- `src/hooks/useUncommittedChanges.ts`

### Baseline screenshots

Capture:

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

### Baseline manual workflow

Run through:

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

### Baseline automated commands

Run before and after each phase:

```bash
bun test src/**/*.test.ts
bun run build
```

If a phase touches Rust/Tauri behavior:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features
```

If local platform setup prevents a command, record the exact failure and continue only if the changed files are unrelated to the failed subsystem.

## Phase 0: Baseline and guardrails

Goal: make later changes measurable and recoverable.

Risk: low.

Do:

1. Add a short UI migration checklist under `docs/internal` or append to this document as progress notes.
2. Capture baseline screenshots and attach/store them according to the project's existing convention.
3. Record current test/build status.
4. Add a small local performance helper only if existing `recordDebugPerf` is not enough for interaction timing.
5. Identify current feature flags/debug flags that can help isolate surfaces:
   - `disableTerminal`
   - `disableEditor`
   - `disableChat`
   - `disableActivityBar`
   - `disableSidebarOverlay`
   - `disableEditorChrome`
   - `disableChatChrome`
6. Confirm which chat panel is the default path, legacy or V3.

Do not:

- Change visual style.
- Change layout.
- Change component semantics.
- Add new UI.

Acceptance criteria:

- Baseline screenshots exist.
- Baseline test/build status is known.
- Rollback approach is documented.
- Current dirty-worktree/user changes are not overwritten.

Rollback:

- Revert the documentation and helper-only PR.

## Phase 1: Token cleanup and visual de-risking

Goal: reduce web-dashboard styling without changing layout or behavior.

Risk: low to medium.

Scope:

- `src/styles/theme.css`
- `src/index.css`
- `src/components/ui/Surface.tsx`
- `src/components/ui/ListRow.tsx`
- Call sites only where class names must be updated mechanically.

Do:

1. Add desktop shell tokens while keeping old tokens mapped:

```css
--surface-app: var(--bg-app);
--surface-pane: var(--bg-panel);
--surface-editor: var(--bg-editor);
--surface-toolbar: var(--bg-panel);
--surface-overlay: var(--bg-surface);
--separator-subtle: var(--border-subtle);
--separator-default: var(--border-default);
--focus-ring: var(--border-focus);
```

2. Add low or no-shadow tokens:

```css
--shadow-persistent: none;
--shadow-popover: var(--shadow-lg);
--shadow-dialog: var(--shadow-xl);
```

3. Add radius tokens by usage:

```css
--radius-pane: 0px;
--radius-control: 4px;
--radius-popover: 6px;
--radius-dialog: 8px;
```

4. Change comments and names away from "card" language where safe.
5. Keep old variables in place so existing components continue working.
6. Change `Surface` variants to internally map persistent variants to flatter styles, but keep the public API during the first pass.
7. Reduce persistent row/card shadows in `ListRow` without changing row height or behavior.

Do not:

- Move panes.
- Change dimensions.
- Delete old tokens yet.
- Touch state hooks.
- Touch Tauri window behavior.

Correctness checks:

- No TypeScript errors.
- Existing UI still renders.
- No unreadable text.
- No invisible borders/focus states.
- No modal/popover loses required elevation.

Acceptance criteria:

- Persistent surfaces look flatter.
- Popovers and dialogs still stand out.
- Existing components do not require broad call-site rewrites.

Rollback:

- Restore token values and primitive class maps.

## Phase 2: Persistent shell from floating cards to integrated panes

Goal: migrate app chrome without changing pane internals.

Risk: medium.

Scope:

- `src/components/Layout.tsx`
- Possibly small CSS additions in `src/index.css` or `src/styles/theme.css`

Feature flag:

- `desktopShellV1`

Do:

1. Introduce shell class helpers or a small local object for layout styles:
   - app root
   - top bar
   - activity rail
   - sidebar pane
   - editor pane
   - chat pane
   - status bar
   - split handle
2. Behind `desktopShellV1`, remove:
   - persistent pane `boxShadow`
   - persistent large border radius
   - rounded status bar card
   - floating activity bar card look
3. Keep:
   - existing dimensions
   - `chatPanelWidth`
   - `terminalHeight`
   - resize handlers
   - editor and chat mounting conditions
   - project state persistence
4. Convert sidebar from floating overlay only if this can be done without touching state logic.
5. Keep old overlay behavior behind the flag until the docked version is stable.

Do not:

- Rewrite `Layout.tsx`.
- Change hook ordering.
- Change terminal drawer mechanics.
- Change chat/editor content.
- Change save-on-close behavior.

Manual tests:

- App launches.
- Resize window from all edges.
- Maximize and fullscreen.
- Toggle sidebar views.
- Resize chat.
- Resize terminal.
- Restart app and confirm restored sizes.
- Send chat message.
- Switch tabs.

Acceptance criteria:

- Shell reads as integrated desktop panes.
- No lost layout persistence.
- No terminal/editor/chat regression.
- Old shell remains reachable until signoff.

Rollback:

- Disable `desktopShellV1`.
- Revert PR if the flag itself caused complexity.

## Phase 3: Semantic activity rail

Goal: fix clickable `div` controls while preserving visual behavior.

Risk: low.

Scope:

- Activity rail section of `src/components/Layout.tsx`
- Optional new primitive: `src/components/ui/RailButton.tsx`

Feature flag:

- `semanticActivityRail`

Do:

1. Create `RailButton` as a thin wrapper around `button`.
2. Support:
   - `label`
   - `selected`
   - `disabled`
   - `badge`
   - `onClick`
   - `children` icon
3. Use `aria-pressed` or `aria-current` consistently.
4. Preserve current labels and titles.
5. Preserve current toggle behavior exactly.
6. Keep the selected indicator visually simple.

Do not:

- Add new rail items.
- Change sidebar state.
- Change git refresh behavior.
- Add animations.

Manual tests:

- Click each rail item.
- Keyboard tab reaches rail buttons.
- Enter/Space activates each rail button.
- Settings opens.
- Git badge still appears.

Acceptance criteria:

- No clickable `div` remains in activity rail.
- Keyboard operation works.
- Visual state is equivalent or clearer.

Rollback:

- Disable `semanticActivityRail` or restore previous rail markup.

## Phase 4: Compact desktop empty states

Goal: remove marketing-page empty states and replace with utility states.

Risk: low to medium.

Scope:

- `src/components/EditorPanel.tsx`
- `src/chat/rendering/ChatViewport.tsx`
- Possibly i18n strings

Feature flag:

- `compactEmptyStatesV1`

Do:

1. Replace large editor welcome layout with compact utility content:
   - workspace name/path if available
   - primary actions: open folder, configure local AI, configure cloud
   - recent/relevant next action if already available
2. Remove or reduce:
   - `text-3xl`
   - large app logo
   - large centered CTA stack
   - large vertical whitespace
   - drop shadows
3. Replace chat empty card with compact state:
   - model readiness
   - short prompt examples only if useful
   - missing configuration state if no model is available
4. Keep all existing click handlers and settings events.

Do not:

- Add a new onboarding flow.
- Fetch new data unless it is already available.
- Change model selection logic.
- Change chat send behavior.

Manual tests:

- Fresh app with no API key.
- App with local AI configured.
- App with cloud API key.
- Empty chat with no messages.
- First message send.

Acceptance criteria:

- No hero-like empty state remains in primary work area.
- Configuration actions still work.
- Empty states do not dominate the desktop shell.

Rollback:

- Disable `compactEmptyStatesV1`.

## Phase 5: App tabs semantics and safer reorder

Goal: improve tabs without breaking tab state.

Risk: medium.

Scope:

- `src/components/AppBar.tsx`
- `src/hooks/useTabManager.ts` only if needed for new reorder commands
- Context menu command definitions if command registry exists by then

Feature flag:

- `semanticAppTabs`

Do:

1. Keep current tab data shape.
2. Convert tab outer interactive element to `button` or a semantically valid tab structure.
3. Add:
   - selected state
   - keyboard focus
   - left/right tab navigation
   - close focused tab shortcut where appropriate
4. Preserve:
   - click to activate
   - close button
   - context menu
   - dirty indicator
   - AI edited indicator
   - virtual changes indicator
   - drag reorder
5. Add non-drag reorder alternatives:
   - move left
   - move right
   - move to beginning
   - move to end
6. Keep drag reorder until keyboard/pointer alternatives are verified.

Do not:

- Change tab IDs.
- Change tab persistence format.
- Change dirty state logic.
- Change editor buffer registry.

Manual tests:

- Open multiple files.
- Switch by mouse.
- Switch by keyboard.
- Close active tab.
- Close inactive tab.
- Reorder by drag.
- Reorder by menu.
- Restart and confirm tab state restores.
- Dirty tab remains dirty.
- AI edited/virtual indicators still appear.

Acceptance criteria:

- Existing tab behavior is preserved.
- Keyboard and single-pointer alternatives exist.
- No editor content loss.

Rollback:

- Disable `semanticAppTabs`.

## Phase 6: Command registry

Goal: centralize commands and shortcuts without changing workflows.

Risk: medium.

Scope:

- New `src/commands` or `src/lib/commands` frontend module.
- `src/components/AppBar.tsx`
- `src/components/ui/ContextMenu.tsx`
- Current shortcut handlers.

Feature flag:

- `commandRegistryV1`

Do:

1. Create a command registry type:

```ts
interface UiCommand {
  id: string;
  label: string;
  group: 'file' | 'edit' | 'view' | 'agent' | 'terminal' | 'help';
  shortcut?: string;
  when?: () => boolean;
  run: () => void | Promise<void>;
}
```

2. Start with commands that already exist:
   - new file
   - close tab
   - close others
   - close all
   - toggle explorer
   - toggle git
   - toggle history
   - toggle fullscreen
   - open settings
3. Generate context menu items from commands only where this reduces duplication.
4. Add a keyboard shortcuts dialog only after commands are stable.
5. Keep legacy handlers until command equivalents are verified.

Do not:

- Add new product features.
- Change backend commands.
- Replace all shortcuts at once.
- Add a command palette in the same PR unless one already exists and only needs registry wiring.

Manual tests:

- Every command works from its old UI location.
- Shortcut executes the same behavior as click.
- Disabled commands are disabled.
- Context menu still opens and closes correctly.

Acceptance criteria:

- First command slice is centralized.
- No command behavior changed.
- Follow-up commands can be added incrementally.

Rollback:

- Disable `commandRegistryV1`.
- Restore direct handlers.

## Phase 7: Modal, menu, dropdown, and focus correctness

Goal: fix correctness of custom overlays before deeper visual changes.

Risk: medium.

Scope:

- `src/components/ui/Modal.tsx`
- `src/components/ui/ContextMenu.tsx`
- `src/components/ui/ThemedDropdown.tsx`
- model selector components
- mention suggestions

Do:

1. Audit each overlay against expected behavior:
   - Escape closes.
   - Click outside closes only when appropriate.
   - Focus moves into dialog/menu.
   - Focus returns to trigger.
   - Arrow keys work for menu/listbox.
   - Tab handling is predictable.
2. Add focus trap for blocking modals.
3. Add labelled dialog semantics.
4. Ensure close buttons meet target-size rules.
5. Ensure popover surfaces use overlay shadow token, not persistent pane shadow token.

Do not:

- Redesign settings in this phase.
- Add new overlay variants.
- Change model data flow.

Manual tests:

- Open/close input modal.
- Open/close confirm modal.
- Open tab context menu.
- Open file menu.
- Open model selector.
- Use mention suggestions by keyboard.

Acceptance criteria:

- Overlay focus behavior is deterministic.
- Keyboard operation works for common paths.
- No persistent focus loss after closing overlays.

Rollback:

- Revert overlay PR. Avoid placing unrelated visual shell changes here.

## Phase 8: Chat V3 rendering performance

Goal: make long chat sessions safe before adding richer agent UI.

Risk: medium to high.

Scope:

- `src/chat/rendering/ChatViewport.tsx`
- `src/chat/rendering/useChatTimelineRows.ts`
- `src/components/ChatMessage.tsx`
- possibly `src/utils/chatTimeline.ts`

Feature flag:

- `chatViewportVirtualizationV1`

Do:

1. Confirm whether legacy `src/components/ChatPanel.tsx` virtualization can be ported.
2. Prefer porting known local logic before introducing a library.
3. Keep active streaming row and recent tail unvirtualized.
4. Collapse or virtualize long tool output blocks.
5. Use `content-visibility: auto` only for inactive, measured blocks where layout stability is acceptable.
6. Keep composer controlled input out of transitions.
7. Use `startTransition` only for non-urgent timeline/history/filter work if profiler shows benefit.
8. Preserve scroll-to-bottom and detached-scroll behavior.

Do not:

- Change message schema.
- Change backend streaming protocol.
- Change approval logic.
- Change markdown parsing in the same PR as virtualization.

Manual tests:

- Short chat.
- Long chat.
- Streaming response.
- User scrolls up while response streams.
- Jump to bottom.
- Pending command approval.
- Long command output.
- Image attachment message if supported.

Performance checks:

- Compare render count before/after.
- Compare DOM node count for long chat.
- Check interaction delay when typing during streaming.
- Check scroll smoothness in long chat.

Acceptance criteria:

- Long chat does not render every historical row.
- Streaming remains visually stable.
- Scroll behavior is preserved.
- No approval or undo regression.

Rollback:

- Disable `chatViewportVirtualizationV1`.

## Phase 9: Settings as desktop preferences

Goal: reduce card-heavy settings layout without changing settings persistence.

Risk: medium.

Scope:

- `src/components/SettingsModal.tsx`
- shared field/list primitives

Do:

1. Keep settings section IDs and saved data unchanged.
2. Convert layout to:
   - left category list
   - right details pane
   - compact section headers
   - standard field rows
3. Replace large instructional cards with inline help or secondary disclosure.
4. Keep Telegram, local AI, cloud, storage, privacy, editor, and about behavior intact.
5. Use existing settings load/save handlers.

Do not:

- Change settings schema.
- Change backend settings commands.
- Combine with command registry or shell migration.
- Add new settings.

Manual tests:

- Open each settings section.
- Save remote AI settings.
- Save local AI settings.
- Toggle privacy/editor settings.
- Configure Telegram flow if feasible.
- Close/reopen settings and verify values.

Acceptance criteria:

- Settings feels like preferences, not a marketing/configuration page.
- All existing settings persist.

Rollback:

- Revert settings PR.

## Phase 10: AI workflow state clarity

Goal: improve trust and control after the shell and performance are stable.

Risk: medium.

Scope:

- chat rendering components
- command approval cards
- uncommitted change actions
- progress/status strips

Do:

1. Define explicit UI states:
   - idle
   - planning
   - editing
   - waiting for approval
   - running command
   - applying patch
   - needs review
   - failed
   - reverted
2. Map existing backend/chat state into these UI states.
3. Keep state labels compact.
4. Put actions next to affected objects:
   - approve/deny next to command
   - accept/reject next to diff/change
   - undo next to completed tool/change
   - open file next to file path
5. Remove decorative AI styling that does not communicate state or risk.
6. Keep explanations collapsed or on demand.

Do not:

- Add new agent capabilities.
- Change tool execution.
- Change patch application.
- Add speculative confidence scores unless backend supplies meaningful data.

Manual tests:

- Planning response.
- Code response.
- Command approval.
- Command denial.
- Command execution failure.
- AI edited file pending review.
- Accept all.
- Reject all.
- Undo tool.

Acceptance criteria:

- User can understand what the agent is doing and what control they have.
- UI state is clearer with less decoration.
- Existing safety controls remain intact.

Rollback:

- Revert AI presentation PR.

## Phase 11: Delete old paths and flags

Goal: remove migration leftovers after stable use.

Risk: medium.

Do only after:

- New shell is stable.
- Semantic rail is stable.
- Semantic tabs are stable.
- Compact empty states are stable.
- Chat virtualization is stable.
- Command registry first slice is stable.

Do:

1. Delete old flagged code paths.
2. Delete unused card/elevation tokens.
3. Delete unused CSS utilities.
4. Delete old component variants that encourage card-like persistent chrome.
5. Remove temporary debug flags introduced for migration.
6. Update internal docs.

Do not:

- Delete existing debug flags that are useful for isolating app surfaces unless they are migration-only.
- Delete legacy chat panel until its replacement has equivalent performance and behavior.

Acceptance criteria:

- Less code than before the migration in shell/style primitives where possible.
- No stale flags.
- No old card shell path remains.

Rollback:

- Restore from the last stable PR if deletion was too aggressive.

## PR size guidelines

Preferred PR shape:

- 1 to 5 files for token/primitive changes.
- 1 component plus tests/manual notes for semantic changes.
- 1 feature flag per risky behavior.
- Under 400 changed lines where practical.

Avoid:

- Reformatting large files.
- Moving files while changing logic.
- Renaming tokens and changing layout in the same PR.
- Styling settings, chat, and layout at the same time.
- Touching Rust/Tauri while changing React visual shell unless required.

## Test checklist per phase

Always:

```bash
bun test src/**/*.test.ts
bun run build
```

When touching command execution, terminal, or Tauri integration:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

When touching Rust logic:

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features
```

Manual smoke test:

- Launch app.
- Open file.
- Type in editor.
- Switch tabs.
- Open/close sidebar.
- Open/resize terminal.
- Send chat message.
- Stop generation.
- Open settings.
- Close app.

Manual regression test for dirty state:

- Edit a file.
- Confirm dirty indicator.
- Close app.
- Choose cancel.
- Close app again.
- Choose save or discard.
- Relaunch and verify state.

## Visual acceptance checklist

For each shell/style PR:

- Text does not overlap.
- Text remains readable in dark and light themes if supported.
- Focus state is visible.
- Selected state is distinct from hover state.
- Disabled state is understandable.
- Hit targets are not too small or too tightly packed.
- Scrollbars do not create phantom bars.
- Terminal and editor are not clipped incorrectly.
- Window resize handles still work.
- Fullscreen and maximized modes still look intentional.

## Accessibility checklist

Minimum:

- Interactive elements are native elements where possible.
- Keyboard can reach controls.
- Enter/Space activate buttons.
- Escape closes menus/dialogs.
- Focus returns after closing overlays.
- Modals have labels.
- Context menus support keyboard navigation.
- Drag operations have non-drag alternatives.
- Targets meet WCAG 2.2 minimum size or spacing.

Preferred:

- App tabs follow tablist expectations.
- File tree follows tree view expectations.
- Dropdowns and mention suggestions follow listbox/combobox expectations.
- Command shortcuts are discoverable.

## Performance checklist

Measure before and after:

- DOM node count in long chat.
- React render count during streaming.
- Composer typing responsiveness during streaming.
- Sidebar toggle time.
- Tab switch time.
- Context menu open time.
- Terminal resize smoothness.
- App startup time if shell changes affect initial rendering.

Preferred tools:

- Existing `recordDebugPerf`.
- React Profiler.
- Browser/WebView performance traces where available.
- Manual frame/lag observation on Linux WebKitGTK, since this repo has WebKit-specific fixes.

## Recommended first five PRs

### PR 1: Baseline and screenshots

Files:

- Documentation only, plus optional measurement helper.

Why first:

- Establishes what must not regress.

### PR 2: Add desktop tokens, keep old token compatibility

Files:

- `src/styles/theme.css`
- `src/index.css` if needed

Why second:

- Gives later PRs stable vocabulary without behavior risk.

### PR 3: Flatten persistent primitive defaults

Files:

- `src/components/ui/Surface.tsx`
- `src/components/ui/ListRow.tsx`
- maybe `src/components/ui/IconButton.tsx`

Why third:

- Removes visual noise across the app without moving layout.

### PR 4: Semantic activity rail behind flag

Files:

- `src/components/Layout.tsx`
- optional `src/components/ui/RailButton.tsx`

Why fourth:

- Small, visible correctness win with limited state risk.

### PR 5: Integrated shell behind flag

Files:

- `src/components/Layout.tsx`
- token CSS if needed

Why fifth:

- This is the first visually meaningful shell migration, but now token and primitive groundwork exists.

## Stop conditions

Pause the migration and fix the underlying issue if:

- Two consecutive PRs need emergency rollback.
- Manual smoke tests start taking too long because behavior is unstable.
- The same component accumulates multiple flags.
- A phase requires broad unrelated refactors.
- Visual changes hide or weaken safety-critical AI controls.
- Performance gets worse and cannot be explained.

## Definition of done for the full migration

The migration is complete when:

- Persistent app shell uses integrated panes and separators.
- Floating card styling remains only for transient overlays or repeated content where appropriate.
- Activity rail and tabs are semantic and keyboard-operable.
- Command discovery is centralized enough to prevent shortcut/menu drift.
- Empty states are compact and workflow-oriented.
- Long chat sessions remain responsive.
- Settings uses a preferences-style layout.
- AI workflow states emphasize review, control, and recovery.
- Migration flags and old shell paths are deleted.
- The codebase has less visual-special-case styling than before the migration.

## Summary

The safe path is not a redesign sprint. It is a series of small removals and substitutions:

1. Baseline.
2. Add safer tokens.
3. Flatten persistent surfaces.
4. Replace non-semantic controls.
5. Move shell layout behind a flag.
6. Centralize commands.
7. Fix overlays and focus.
8. Make chat scalable.
9. Simplify settings.
10. Clarify AI state.
11. Delete old paths.

This sequencing protects the high-risk parts of the app: editor buffers, terminal sessions, project state, command execution, chat approvals, and uncommitted-change review. It also keeps each rollback small enough that the UI cannot become unrecoverable unless several independent guardrails are ignored.
