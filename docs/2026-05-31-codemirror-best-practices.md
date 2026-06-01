# CodeMirror, React, and Editor State Best Practices

Date: 2026-05-31

## Context

This document was written after a critical editor/data-loss investigation in ZaguanBlade. The recent symptoms were severe:

- Opening or switching file tabs could show the previous tab's content.
- The incorrect content appeared to live in the editor buffer rather than on disk.
- Shutdown could prompt to save the incorrect buffer.
- Saving could overwrite the target file with content from another file.
- Rejecting model-generated edits could leave rejected content in the editor and later save it back to disk.

The immediate patches added file-path ownership to editor state updates and made reject/revert reloads authoritative. Those patches are pragmatic guards, but they do not answer the deeper architecture question:

> Are we letting CodeMirror be the editor, or are we pushing React into live editor state ownership where it does not belong?

This document gathers CodeMirror 6 and React integration best practices and applies them to ZaguanBlade's editor architecture.

## Sources reviewed

Primary and high-signal sources:

- CodeMirror System Guide: https://codemirror.net/docs/guide/
- CodeMirror Reference Manual: https://codemirror.net/docs/ref/
- CodeMirror 5 to 6 Migration Guide: https://codemirror.net/docs/migration/
- CodeMirror discussion, "Proper way to listen for changes": https://discuss.codemirror.net/t/codemirror-6-proper-way-to-listen-for-changes/2395
- CodeMirror discussion, "Re-render view on state.doc update": https://discuss.codemirror.net/t/re-render-view-on-state-doc-update/3846
- Trevor Harmon, "CodeMirror and React": https://thetrevorharmon.com/blog/codemirror-and-react/

The CodeMirror docs and maintainer discussion should be treated as authoritative. Blog guidance should be treated as useful implementation experience, not a replacement for CodeMirror's own model.

## Core CodeMirror 6 principles

### 1. CodeMirror has a functional core and imperative shell

CodeMirror's architecture is explicitly described as a functional core wrapped by an imperative view.

Important consequences:

- `EditorState` is immutable.
- The document is part of immutable editor state.
- You do not mutate state objects directly.
- Changes happen by creating and dispatching transactions.
- `EditorView` is the imperative shell that receives transactions and updates DOM/state.

Implication for ZaguanBlade:

React should not mutate or shadow CodeMirror state as if CodeMirror were a textarea. The reliable integration point is `EditorView.dispatch(...)`, not React repeatedly pushing a `content` prop and hoping all mirrors stay synchronized.

### 2. All regular editor updates should go through `EditorView.dispatch`

The reference manual says regular state updates should go through `dispatch`. This includes:

- Document edits
- Selection changes
- State effects
- Configuration reconfiguration

For example, replacing content should be done as a transaction:

```ts
view.dispatch({
  changes: {
    from: 0,
    to: view.state.doc.length,
    insert: nextContent,
  },
});
```

For a true file switch or full reset, CodeMirror recommends creating a new state instead of trying to preserve inappropriate previous state:

```ts
view.setState(EditorState.create({
  doc: nextContent,
  extensions,
}));
```

The choice matters:

- **Same file, external edit:** prefer dispatching a document change, ideally minimal if feasible.
- **Different file:** reset editor state so undo history, selection, decorations, and file-specific extension state do not bleed across files.

### 3. Do not directly manipulate CodeMirror's DOM

CodeMirror owns its DOM. The docs are explicit that user code should not directly modify the editor content DOM. CodeMirror may immediately revert such changes.

Use:

- Transactions for content/selection/state updates.
- Decorations for visual styling.
- State fields/effects for custom state.
- View plugins for view-level behavior.

Implication for ZaguanBlade:

Diff highlights, removed-line widgets, AI glow, line highlights, and similar editor visuals belong in CodeMirror extensions/decorations, not React overlays that try to track document lines independently.

### 4. CodeMirror does not render the whole document

CodeMirror uses a viewport model for performance. It renders visible content plus a margin, not the entire document.

Implications:

- Avoid React-driven per-line rendering for editor content.
- Prefer CodeMirror decorations, range sets, and visible-range-aware logic.
- Expensive diff/decorations should scale with visible ranges or bounded parsed diff data.
- Avoid querying layout outside the viewport.

### 5. Update listeners are the intended way to observe editor changes

The CodeMirror maintainer recommends `EditorView.updateListener.of(update => ...)` as the shorthand way to listen for changes.

Use it to observe settled CodeMirror updates:

```ts
EditorView.updateListener.of((update) => {
  if (!update.docChanged) return;

  const text = update.state.doc.toString();
  // Report lightweight metadata or schedule persistence.
});
```

Important nuance:

Do not put side effects inside pure CodeMirror state-field update functions. Use update listeners/view plugins for observing completed updates and reporting outward.

## React integration principles

### 1. Prefer CodeMirror as the source of truth for the live buffer

The strongest React-specific guidance from the research is:

> As much as possible, CodeMirror should be the source of truth, with React being a consumer of the state.

For ZaguanBlade, that means:

- CodeMirror owns the mounted active document text.
- React owns app-level routing, file identity, tabs, prompts, commands, and persistence orchestration.
- React should not continuously mirror every keystroke as authoritative `content` state.

A healthy split:

| Concern | Owner |
| --- | --- |
| Live active text buffer | CodeMirror |
| User text input | CodeMirror transactions |
| Selection/cursor/scroll | CodeMirror |
| Undo history | CodeMirror, per file/editor state |
| Diff decorations | CodeMirror extensions/state fields |
| Active file path | React/Layout/editor shell |
| Tab list | React or backend tab service |
| Dirty metadata for tab badges | Derived from CodeMirror/buffer registry |
| Save/revert/accept/reject commands | React orchestrates, CodeMirror executes view update |
| Shutdown prompt list | App state derived from per-file dirty registry |

### 2. Avoid fully controlled React editor content unless there is a very strong reason

A normal React controlled input model looks like this:

1. User types.
2. React state updates.
3. React re-renders with new `value`.
4. DOM follows React.

This is not a good default mental model for CodeMirror. CodeMirror already has its own state lifecycle, transactions, selection mapping, undo history, composition handling, viewport rendering, and plugin system.

Risks of over-controlling CodeMirror from React:

- Keystroke-level React render pressure.
- Stale closures and debounced updates.
- Duplicate sources of truth.
- Incorrect attribution when active tab changes before callbacks flush.
- Undo/selection/history loss when replacing full documents unnecessarily.
- Incorrect dirty prompts from stale React mirrors.

### 3. Memoize extensions and configuration aggressively

React re-renders can accidentally rebuild CodeMirror extensions. Guidance from practitioners is to memoize more than usual around CodeMirror.

Use stable values for:

- Extension arrays
- Theme extensions
- Language extensions
- Update listeners
- Compartments
- Callback bridges

Avoid inline anonymous objects/functions in places that cause CodeMirror reconfiguration unless intentionally dynamic.

### 4. Use Compartments for dynamic configuration

CodeMirror's recommended way to change configuration after editor creation is `Compartment`.

Good uses:

- Theme changes
- Language mode changes
- Read-only mode
- Line wrapping
- Linting toggles
- Feature flags for editor extensions

Pattern:

```ts
const compartment = new Compartment();

const extensions = [
  compartment.of(initialExtension),
];

view.dispatch({
  effects: compartment.reconfigure(nextExtension),
});
```

Do not rebuild the whole editor just to toggle a configuration option unless the file/editor state itself should reset.

### 5. Use StateEffects and StateFields for editor-owned custom state

React-to-CodeMirror updates should enter through transactions, usually as effects:

```ts
const setDiffState = StateEffect.define<DiffState | null>();

view.dispatch({
  effects: setDiffState.of(nextDiff),
});
```

CodeMirror-to-React updates should be observed through update listeners or view plugins after the state settles.

This is directly relevant to ZaguanBlade's diff UI. Diff metadata belongs in a CodeMirror state field/effect/decorations path, not in React attempting to coordinate visual editor state line-by-line.

## File switching and document replacement

### Same file vs different file must be explicit

CodeMirror's guidance distinguishes ordinary document updates from full state resets.

For ZaguanBlade, file identity must be part of the editor shell contract.

When `activeFile` changes:

- Treat it as a different editor document.
- Reset or swap CodeMirror state for that file.
- Do not preserve undo history from the previous file.
- Do not preserve diff/decorations from the previous file unless explicitly associated with the new file.
- Do not let late async events for file A update file B.

When same file content changes externally:

- If the file is clean, dispatch an external update or reset baseline.
- If the file is dirty, decide explicitly whether to prompt, ignore, merge, or mark conflict.
- Do not silently overwrite local edits.

### Avoid full replacement for every update

The maintainer warned that replacing the whole document for arbitrary external updates loses state and wastes CPU. It can lose selection, undo history, and extra document-related state.

Use full replacement/reset for:

- Loading a different file.
- Reverting a file where undo/history should not preserve rejected content.
- Opening initial content.
- Recovery from corrupted/inconsistent state.

Prefer minimal dispatch changes for:

- Model edits if precise changes are known.
- Formatter output if diff can be computed safely.
- External file changes while the file remains open and clean.

## Dirty state and save prompts

### Dirty state should be derived, not guessed

The current bug class appears when dirty state becomes a manually propagated boolean that can drift away from the real buffer.

Recommended invariant:

```ts
isDirty(filePath) === currentDocument(filePath) !== cleanBaseline(filePath)
```

Where:

- `currentDocument(filePath)` comes from CodeMirror if mounted, or an explicit per-file draft registry if inactive.
- `cleanBaseline(filePath)` is the last known saved/on-disk content for that file.

Do not rely on a standalone `isDirty` boolean unless it can be recomputed or verified.

### Shutdown should never save unverified cross-file content

Before writing a file during shutdown, validate:

- The tab/path exists.
- The draft is explicitly associated with that exact normalized path.
- The draft was produced by that file's editor state or per-file registry.
- The dirty comparison is against that file's baseline.

A good shutdown save item shape is path-keyed:

```ts
type DirtySaveCandidate = {
  path: string;
  baselineHash?: string;
  draft: string;
  source: 'mounted-codemirror' | 'inactive-draft-registry';
};
```

Avoid constructing shutdown save payloads from "currently active tab plus whatever snapshot callback returned" unless the snapshot carries and matches `filePath`.

## Recommended architecture for ZaguanBlade

### Target design: editor shell plus per-file buffer registry

The cleanest direction is not to make React control CodeMirror text. Instead, introduce a small editor buffer service/registry with explicit file keys.

Conceptual model:

```ts
type FileBufferState = {
  path: string;
  cleanContent: string;
  draftContent?: string;
  dirty: boolean;
  version: number;
  lastDiskMtimeMs?: number;
};
```

For the active mounted file:

- CodeMirror owns `draftContent` live.
- Registry stores metadata and inactive drafts.
- Registry updates are keyed by normalized `path`.

For inactive dirty files:

- Store draft in registry if unsaved tab switching is supported.
- Restore by file path when remounting/switching back.

For clean inactive files:

- Do not store duplicate full content unless needed for performance/recovery.
- Reload from disk or cache baseline by path.

### Recommended ownership boundaries

#### `Layout`

Should own:

- Tab list and active tab id.
- App shell layout.
- Shutdown orchestration.
- Mapping editor state updates to tab metadata by file path.

Should not own:

- Live CodeMirror text for the mounted editor.
- Editor selection/scroll/undo state.
- Per-keystroke document contents unless preserving inactive dirty drafts.

#### `EditorPanel`

Should own:

- Active file identity passed to editor shell.
- Loading file content.
- Save/revert command orchestration.
- Bridging CodeMirror events to app-level metadata.

Should not own multiple redundant mirrors of the same text unless each has a clear invariant.

Potentially remove or simplify:

- `contentOwnerPath`
- `liveContentRef`
- `baseContentRef`
- `savedContent`/`draftContent` prop mirroring
- `externalContentVersionRef`

But only after replacing them with a smaller explicit model.

#### `CodeEditor`

Should own:

- `EditorView` lifecycle.
- CodeMirror state creation/reset.
- Transactions for document changes.
- Decorations and visual editor features.
- Update listener for reporting document changes.

Should expose imperative commands through a ref:

```ts
type CodeEditorHandle = {
  getContent(): string;
  replaceDocument(input: {
    path: string;
    content: string;
    resetHistory: boolean;
    reason: 'open' | 'reload' | 'revert' | 'external-clean-update';
  }): void;
  setDiff(diff: DiffState | null): void;
  focus(): void;
};
```

### Recommended invariants

These should become explicit rules and eventually tests.

1. **Path ownership:** no content state update may be applied to a tab unless `normalize(update.path) === normalize(tab.path)`.
2. **File switch reset:** switching from file A to file B must reset CodeMirror document identity before file B can be edited.
3. **No previous-file save:** shutdown save candidates must be path-keyed and must not derive from another file's active snapshot.
4. **Dirty derivation:** dirty is derived from draft vs baseline, not manually trusted.
5. **Reject is authoritative:** rejecting a model edit updates baseline and document to reverted disk/snapshot content.
6. **Accept is clean:** accepting a model edit removes pending diff UI without marking the file dirty solely because an AI changed it on disk.
7. **External stale read safety:** a late read for file A cannot update file B, and a stale read for the same file cannot overwrite a newer document version.
8. **No React keystroke control:** React does not push a new `content` prop into CodeMirror on each keystroke.

## Performance recommendations

### 1. Avoid `doc.toString()` on every transaction unless necessary

Calling `update.state.doc.toString()` copies the whole document. For small files this is fine; for large files it can be expensive.

Better patterns:

- On every `docChanged`, mark dirty/version changed.
- Debounce full text extraction for persistence/sync.
- Extract full text on save, shutdown snapshot, or scheduled sync.
- Use transaction changes for incremental systems where practical.

### 2. Bound expensive editor aids by document size

Features like diff decorations, syntax-aware tooling, AI glow, linting, semantic overlays, and markdown preview should have clear thresholds.

ZaguanBlade already has some size guards. Keep and expand that approach.

Recommended guard categories:

- Max full-document text extraction frequency.
- Max diff source size for rich inline decoration.
- Max line count for complex widgets.
- Visible-range-only rendering for expensive visual features.
- Fallback simplified diff mode for large diffs.

### 3. Use visible ranges for decoration-heavy features

CodeMirror exposes viewport/visible ranges because it does not render the whole document. Decoration builders should avoid doing unnecessary work for invisible content when possible.

### 4. Preserve scroll/selection intentionally

Do not preserve scroll and selection by accident across files.

- Same file clean update: preserve selection/scroll when reasonable.
- Different file: restore per-file selection/scroll if stored by path, otherwise use default.
- Revert/reject: choose intentionally whether to preserve scroll; do not preserve undo history containing rejected content.

### 5. Use compartments for dynamic editor settings

Line wrapping, read-only state, theme, language, and optional extensions should be compartment-driven rather than full editor recreation.

## Proposed tomorrow plan

### Phase 1: Draw the current state graph

Document every current holder of editor content/dirty state:

- CodeMirror `view.state.doc`
- `EditorPanel.content`
- `contentOwnerPath`
- `liveContentRef`
- `baseContentRef`
- `externalContentVersionRef`
- tab `savedContent`
- tab `draftContent`
- tab `isDirty`
- pending content-state refs/timers
- shutdown snapshot ref
- uncommitted-change diff state

For each one answer:

- What invariant does it represent?
- Who writes it?
- Who reads it?
- Is it path-keyed?
- Can it lag behind CodeMirror?
- Can it be removed?

### Phase 2: Define the target minimal model

Before editing code, write down the replacement model.

Candidate minimal model:

- CodeMirror owns active text.
- A path-keyed buffer registry owns clean baseline and inactive dirty drafts.
- Tabs own only metadata and dirty badge status derived from registry.
- Layout never stores content except through path-keyed registry data.

### Phase 3: Add regression tests before refactor

High-value tests/scenarios:

1. Open file A, then file B. B must never show A's content.
2. Open file A, edit A dirty, switch to B, close app. Save prompt must mention A only.
3. Open file A, delayed content event for A arrives while B active. B must not change.
4. Reject model edit in A while A dirty from model content. A returns to baseline and shutdown is clean.
5. Accept model edit in A. Diff disappears and app does not prompt to save solely because model wrote to disk.
6. Dirty inactive tab preserves its own draft and restores it when returning to that tab.
7. Same filename in different directories does not collide.
8. Absolute vs relative path representations normalize correctly.

### Phase 4: Refactor one boundary at a time

Avoid a big-bang rewrite.

Suggested order:

1. Create path-keyed buffer/dirty registry.
2. Make shutdown consume registry-derived save candidates.
3. Make `EditorPanel` report only path-keyed metadata.
4. Make `CodeEditor` expose explicit document commands.
5. Remove redundant React mirrors.
6. Simplify reject/accept reload flow to explicit commands.
7. Simplify diff propagation into CodeMirror state/effects.

### Phase 5: Remove code, do not add cleverness

For every line added, identify which existing line/state/ref can be deleted.

Good refactor outcome:

- Fewer content mirrors.
- Fewer timers that affect correctness.
- Fewer places where `isDirty` is manually assigned.
- More path-keyed invariants.
- More CodeMirror transactions/effects.
- Less React-controlled editor text.

## Red flags in current architecture

These deserve scrutiny before further patching:

- `contentOwnerPath` suggests React state can contain content for a different active file.
- `savedContent` and `draftContent` on tabs duplicate editor/buffer state.
- `lastPropagatedDirtyRef` can drift from actual CodeMirror content.
- Debounced content-state propagation can fire after active tab changes.
- Shutdown merges active/pending snapshots into tabs, which is dangerous unless strictly path-keyed.
- Full document replacement may be used in cases where CodeMirror transactions or `setState` with intentional reset semantics would be clearer.
- File read response correlation and reload triggers are compensating for unclear ownership.

## What code deserves to exist?

A useful test for each editor-state line:

1. Does this line define a single authoritative source of truth?
2. Is the state keyed by normalized file path if it can outlive a render?
3. Can this state become stale relative to CodeMirror?
4. If stale, is it harmless metadata or can it write to disk?
5. Can this be derived instead of stored?
6. Does CodeMirror already provide a better primitive?
7. Is this preserving user data, or just making a race less likely?

If a line stores full document content outside CodeMirror, it needs a strong reason:

- Inactive dirty draft preservation.
- Clean baseline comparison.
- Shutdown recovery.
- Explicit persistence cache.

Otherwise it is probably accidental complexity.

## Recommended north-star design

The editor should feel like this:

```txt
React/Layout
  owns: tabs, active path, commands, prompts
  does not own: live editor text

EditorPanel
  owns: file loading/saving orchestration for active path
  bridges: CodeMirror events <-> path-keyed app metadata

CodeEditor
  owns: EditorView, live doc, selection, undo, decorations
  accepts: explicit commands by path/reason

BufferRegistry
  owns: path-keyed baseline/draft/dirty metadata
  feeds: tab badges, shutdown save candidates, inactive draft restore
```

In this model, the following bug should become structurally impossible:

> File A content appears in file B and then shutdown saves it to B.

Because content/draft/baseline data cannot exist without a normalized path key, and save candidates are derived from that key.

## Immediate recommendation

Do not keep adding ad hoc guards to the current editor state graph tonight.

Tomorrow, start with a short design session and decide:

1. Is CodeMirror the live buffer owner? Recommended answer: yes.
2. Do we support inactive unsaved drafts? If yes, use a path-keyed registry.
3. Are tabs allowed to store full content? Recommended answer: only as derived registry snapshots, not as primary state.
4. Should file switch use `setState` rather than document replacement transaction? Recommended answer: yes, for true file identity changes.
5. Should reject/revert reset history? Recommended answer: likely yes, rejected model content should not remain in undo history unless intentionally desired.

The guiding principle should be subtraction:

> Remove ambiguous state ownership. Let CodeMirror be the editor. Let React orchestrate the app.
