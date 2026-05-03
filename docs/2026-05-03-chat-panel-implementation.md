# Chat Panel Implementation Plan

Date: 2026-05-03
Companion to: `docs/2026-05-03-new-chat-panel.md`

## 0. Purpose

Turn the design in `docs/2026-05-03-new-chat-panel.md` into a concrete, step-by-step implementation plan with explicit file paths, APIs, feature flags, rollout phases, and validation gates. The plan is written so it can be executed incrementally without breaking the current chat panel.

## 1. Ground Rules

- **No parallel forks.** Build the new chat under `src/chat/` alongside the old panel. Gate cutover behind a single runtime feature flag, not scattered conditionals.
- **Ship Phase-by-Phase.** Each phase ends in a green `bun run build`, `bun test`, and `cargo check -p zblade`. No half-wired phases.
- **No protocol changes.** Consumption of BCP events only. If a real protocol gap appears, stop and raise a BCP RFC per `docs/internal/BLADE_CHANGE_PROTOCOL.md`.
- **Preserve existing behavior.** Reasoning, tools, approvals, screenshots, queueing, planning mode, model selection, history, todos, and run status all remain functional at every phase boundary.
- **Rust-first where it pays.** Keep event normalization in TypeScript (near the UI), but lean on existing Rust scheduler/sequencing. No new JS-side sequencing heuristics.
- **Delete on exit.** Phase 7 physically removes legacy code; no long-lived dead code after cutover.

## 2. Feature Flag

Single runtime flag: `chat.panel.v3` (boolean), default `false`.

- Exposed through the existing remote settings `configuration` slice (see `src/contexts/ThemeContext.tsx` precedent).
- Read once at app load, stored in a small `ChatPanelFlagContext`.
- Dev override via `localStorage.setItem('chat.panel.v3', '1')`.
- `ChatPanel` legacy stays mounted when flag is `false`. New panel mounts when `true`.

Remove flag and both branches in Phase 7.

## 3. Target File Layout

Create new tree under `src/chat/`:

```txt
src/chat/
  protocol/
    chatEventBridge.ts
    chatProtocolReducer.ts
    chatProtocolTypes.ts
    legacyHistoryMigration.ts
  store/
    chatStore.ts
    selectors.ts
    useChatActions.ts
    useChatSelectors.ts
  rendering/
    ChatPanel.tsx
    ChatHeader.tsx
    ChatViewport.tsx
    VirtualMessageList.tsx
    MessageRow.tsx
    UserMessage.tsx
    AssistantTurn.tsx
    AssistantText.tsx
    ReasoningSection.tsx
    ToolTimeline.tsx
    ToolCallRow.tsx
    CommandResultRow.tsx
    ApprovalInlineCard.tsx
    SystemMessage.tsx
    RunStatusDock.tsx
    TaskStrip.tsx
    QueueStrip.tsx
    FloatingJumpToBottomButton.tsx
  composer/
    Composer.tsx
    ComposerTextarea.tsx
    ComposerToolbar.tsx
    ModelSelectButton.tsx
    MentionSuggestions.tsx
    AttachmentControls.tsx
    ScreenshotPickerLauncher.tsx
    useComposerAttachments.ts
    usePathSuggestions.ts
  markdown/
    PlainStreamingText.tsx
    MarkdownLite.tsx
    MarkdownFull.tsx
    codeBlockLazy.ts
  debug/
    chatPerf.ts
```

Legacy files stay until Phase 7:

- `src/components/ChatPanel.tsx`
- `src/components/ChatMessage.tsx`
- `src/hooks/useChatV2.ts`
- `src/components/MarkdownRenderer.tsx`
- `src/components/CommandCenter.tsx`
- `src/components/ToolCallDisplay.tsx`
- `src/utils/chatTimeline.ts`
- `src/utils/messageBlocks.ts`

## 4. Normalized State Shape

Defined in `src/chat/protocol/chatProtocolTypes.ts`.

```ts
export type MessageId = string;
export type ToolCallId = string;

export type AssistantPart =
  | { kind: 'reasoning'; id: string; text: string; status: 'streaming' | 'done' }
  | { kind: 'text'; id: string; text: string; status: 'streaming' | 'done' }
  | { kind: 'tool_call'; id: ToolCallId }
  | { kind: 'command_result'; toolCallId: ToolCallId };

export interface ChatMessageEntity {
  id: MessageId;
  role: 'user' | 'assistant' | 'system';
  createdAt: number;
  updatedAt: number;
  status: 'streaming' | 'complete' | 'error' | 'stopped';
  user?: { text: string; images?: ImageRef[]; mentions?: Mention[] };
  assistant?: { parts: AssistantPart[] };
  system?: { kind: SystemMessageKind; text: string };
  errorCode?: string;
}

export interface ToolCallEntity {
  id: ToolCallId;
  messageId: MessageId;
  name: string;
  rawArguments: string;
  parsedArguments: Record<string, unknown> | null;
  display: {
    title: string;
    path?: string;
    shortPath?: string;
    command?: string;
    cwd?: string;
    query?: string;
  };
  status: 'pending' | 'executing' | 'complete' | 'error' | 'skipped';
  resultPreview?: string;
}

export interface ApprovalEntity {
  toolCallId: ToolCallId;
  messageId: MessageId;
  kind: 'run_command' | 'edit' | 'delete' | 'other';
  payload: unknown;
  createdAt: number;
}

export interface CommandResultEntity {
  toolCallId: ToolCallId;
  output: string;
  truncated: boolean;
  exitCode: number | null;
  durationMs: number | null;
}

export interface ChatStoreState {
  conversationId: string | null;
  messageOrder: MessageId[];
  messagesById: Record<MessageId, ChatMessageEntity>;
  toolCallById: Record<ToolCallId, ToolCallEntity>;
  commandResultByToolCallId: Record<ToolCallId, CommandResultEntity>;
  pendingApprovalByToolCallId: Record<ToolCallId, ApprovalEntity>;
  activeTurnId: MessageId | null;
  activeApprovalToolCallId: ToolCallId | null;
  activeTodos: TodoItem[];
  queuedRequests: QueuedRequest[];
  runStatus: RunStatus;
}
```

The store is a Zustand-like subscription boundary in `chatStore.ts` using `useSyncExternalStore`. No external dep needed if Zustand is not already present; otherwise use the existing store library.

## 5. Event Pipeline

### 5.1 `chatEventBridge.ts`

- Subscribes to BCP events via existing `src/services/bladeEvents.ts` helpers (`subscribeBladeNestedEventType`).
- Decodes to internal `ChatAction` union.
- Dispatches actions to the store reducer.
- Owns per-message `MessageBuffer` instances (reused from `src/utils/eventBuffer.ts`) outside React.
- Owns a single `requestAnimationFrame` flush loop for text/reasoning deltas.
- When hidden (`document.visibilityState === 'hidden'`), falls back to `setTimeout(..., 250)`.

### 5.2 `chatProtocolReducer.ts`

- Pure `(state, action) => state`.
- Handles: `MessageCreated`, `TextDelta`, `ReasoningDelta`, `ToolCallStarted`, `ToolCallArgumentsDelta`, `ToolCallComplete`, `ToolCallError`, `ToolCallSkipped`, `CommandResult`, `ApprovalRequested`, `ApprovalResolved`, `MessageCompleted`, `MessageStopped`, `MessageError`, `TodoUpdated`, `QueueUpdated`, `RunStatus`, `HistoryLoaded`.
- Deltas mutate the last matching `AssistantPart` of the same kind if it is still `streaming`; otherwise append a new part. This removes `content_before_tools` / `content_after_tools` logic entirely.
- Tool argument parsing happens here, once, when `ToolCallComplete` fires or periodically when `rawArguments` becomes valid JSON.
- No DOM references, no React imports.

### 5.3 `useChatActions.ts`

Small hook exposing imperative actions to UI:

```ts
sendMessage(input: ComposerInput): Promise<void>;
stopGeneration(): void;
approveToolCall(toolCallId: ToolCallId, decision: ApprovalDecision): void;
skipToolCall(toolCallId: ToolCallId): void;
undoLastTurn(): void;
selectModel(modelId: string): void;
loadConversation(id: string): Promise<void>;
startNewConversation(): void;
setPlanningMode(enabled: boolean): void;
```

Each action calls existing Tauri commands; no duplicate backend logic.

### 5.4 `useChatSelectors.ts`

Expose narrow selectors, each using `useSyncExternalStore`:

```ts
useMessageOrder(): MessageId[];
useMessage(id: MessageId): ChatMessageEntity | undefined;
useAssistantParts(id: MessageId): AssistantPart[] | undefined;
useToolCall(id: ToolCallId): ToolCallEntity | undefined;
useCommandResult(id: ToolCallId): CommandResultEntity | undefined;
useApprovalForToolCall(id: ToolCallId): ApprovalEntity | undefined;
useActiveTurnId(): MessageId | null;
useRunStatus(): RunStatus;
useActiveTodos(): TodoItem[];
useQueuedRequests(): QueuedRequest[];
useConversationId(): string | null;
```

Each selector uses a reference-stable snapshot so unrelated updates do not re-render.

## 6. Rendering Rules

- `ChatPanel` composes layout only. No state derivation. No `useEffect` for scroll or row geometry.
- `VirtualMessageList` uses `@tanstack/react-virtual` with `measureElement`. It subscribes to `messageOrder` only.
- `MessageRow` subscribes to a single message entity.
- `AssistantTurn` subscribes to its `assistant.parts` array reference. Because deltas mutate the last streaming part in place and we return a new `parts` array only when structure changes, non-active rows do not re-render on streaming.
- `AssistantText` subscribes to a single `AssistantPart` by `id`. While `status === 'streaming'`, it renders through `PlainStreamingText`; once `done`, it promotes to `MarkdownLite` or `MarkdownFull`.
- `ReasoningSection` collapsed by default when `done`; auto-expanded while `streaming` with recent content; manual toggle always wins.
- `ToolTimeline` renders an ordered list of `{ tool_call, command_result }` pairs drawn from `parts`.
- `ApprovalInlineCard` is shown inside `ToolCallRow` when `useApprovalForToolCall(id)` returns an entity.
- `RunStatusDock` is the only place with continuous animation while idle-streaming (single spinner + rotating verb per existing memory).
- `TaskStrip` and `QueueStrip` subscribe only to their slices.

## 7. Streaming Budget

Implemented in `chatEventBridge.ts`:

```ts
const pending = new Map<MessageId, { text?: string; reasoning?: string }>();
let rafHandle: number | null = null;
let hiddenTimer: number | null = null;

function schedule() {
  if (document.visibilityState === 'hidden') {
    if (hiddenTimer == null) hiddenTimer = window.setTimeout(flush, 250);
    return;
  }
  if (rafHandle == null) rafHandle = requestAnimationFrame(flush);
}
```

Rules:

- Append raw deltas to the per-message buffers synchronously.
- Flush merges buffered deltas into the store in a single action per message.
- Tool argument deltas are coalesced the same way but only re-parsed on `ToolCallComplete`.
- Never flush from inside a reducer or selector.

## 8. Scroll Model

In `ChatViewport.tsx`:

```ts
type ScrollMode = 'following' | 'detached';
```

- One `IntersectionObserver` watching a bottom sentinel drives `following` vs `detached`.
- `FloatingJumpToBottomButton` visible only when `detached`.
- New user message via `useChatActions.sendMessage` force-sets `following` and calls virtualizer `scrollToIndex(last, { align: 'end' })`.
- Streaming deltas never call `scrollIntoView` directly; instead, while `following`, the virtualizer `scrollToIndex` is called at most once per RAF when the last row grows.
- No `behavior: 'smooth'` during streaming.

## 9. Markdown Strategy

Modes in `src/chat/markdown/`:

- `PlainStreamingText`: renders text in a `<div>` with `white-space: pre-wrap`. Escapes HTML. No tokenization.
- `MarkdownLite`: small custom renderer covering paragraphs, inline code, bold/italic, links, lists, blockquotes. No fenced code blocks. Targeted for < 1ms parse on typical messages.
- `MarkdownFull`: uses existing `react-markdown` chain for completed text that contains fenced code or tables.
- `codeBlockLazy.ts`: dynamic `import('react-syntax-highlighter/...')` on visible, completed code blocks only. Skip for blocks > 30 KB or > 800 lines; render plain `<pre><code>` with copy button.

Promotion logic in `AssistantText`:

1. If part `status === 'streaming'` -> `PlainStreamingText`.
2. If `done` and no fenced blocks/tables -> `MarkdownLite`.
3. Else -> `MarkdownFull`.

## 10. Tool Timeline Details

- `ToolCallRow` renders only `ToolCallEntity.display` + `status` + optional `resultPreview`.
- No JSON parsing or friendly-name lookup in render. All of that is done once in the reducer when the call is registered or completes.
- Completed tools collapse after a single CSS transition; no per-row timers. Use an `:is([data-status="complete"])` CSS rule with `transition: opacity`.
- Approvals render inline (no separate out-of-band section) via `ApprovalInlineCard`.

## 11. Composer Plan

Split `CommandCenter.tsx` as follows:

| New file | Responsibility |
|---|---|
| `Composer.tsx` | Layout, state coordination |
| `ComposerTextarea.tsx` | Text, selection, autosize only |
| `ComposerToolbar.tsx` | Send/stop, mode toggle, attach buttons |
| `ModelSelectButton.tsx` | Model list + select; subscribes to model store |
| `MentionSuggestions.tsx` | Mention popover; debounced |
| `AttachmentControls.tsx` | Image/file attachments |
| `ScreenshotPickerLauncher.tsx` | Existing screenshot flow entry |
| `useComposerAttachments.ts` | Attachment state + validation |
| `usePathSuggestions.ts` | Debounced path suggestion fetch with AbortController |

The composer never subscribes to message state. It only reads `runStatus` for send/stop toggle.

## 12. Legacy History Migration

`legacyHistoryMigration.ts`:

- Accepts existing `ChatMessage[]` as loaded by current history flow.
- Produces normalized `ChatStoreState` patches: messages, tool calls, command results.
- Runs once per history load; not on every render.
- Keeps image preview reconciliation from the current code, isolated to this module.

## 13. Phased Rollout

### Phase 1 — Instrumentation (baseline)

Files: `src/chat/debug/chatPerf.ts`, small hooks added to existing `src/components/ChatPanel.tsx`, `src/components/ChatMessage.tsx`, `src/hooks/useChatV2.ts`.

Counters (per second, sampled):

- stream events received
- stream flushes
- active-row renders
- message-list renders
- markdown parse time (ms/render)
- virtualizer measure count (current + future)
- scroll mode transitions
- mounted row count

Expose via a hidden `window.__chatPerf` for ad-hoc inspection. No UI surface.

Exit gate: baseline numbers recorded in `docs/bugs/` or attached to this plan as a follow-up note.

### Phase 2 — Store + Bridge Behind Flag

Files: `src/chat/protocol/*`, `src/chat/store/*`, `src/chat/debug/chatPerf.ts` wired.

- Implement reducer with full action coverage including stopped generation, context-length and message-too-large errors, skipped tools, todo updates, queue updates, history loaded.
- Feed the same Tauri events into both the legacy hook and the new bridge when `chat.panel.v3 === true` (dual dispatch), but do not render from it yet.
- Add reducer unit tests covering every action and every AssistantPart transition:
  - `src/chat/protocol/chatProtocolReducer.test.ts`
  - `src/chat/store/chatStore.test.ts`
- Validate parity with a scripted conversation replay test: load N recorded event sequences; assert final state matches expected snapshot.

Exit gate: reducer tests green, dual dispatch causes no regressions in legacy panel.

### Phase 3 — Render New Messages

Files: `src/chat/rendering/ChatPanel.tsx`, `MessageRow.tsx`, `UserMessage.tsx`, `AssistantTurn.tsx`, `AssistantText.tsx`, `ReasoningSection.tsx`, `ToolTimeline.tsx`, `ToolCallRow.tsx`, `CommandResultRow.tsx`, `ApprovalInlineCard.tsx`, `SystemMessage.tsx`, `RunStatusDock.tsx`, `TaskStrip.tsx`, `QueueStrip.tsx`.

- Initial version uses a plain non-virtualized list.
- Reuses existing `CommandCenter` via an adapter while composer split is pending.
- When `chat.panel.v3` is on, render the new tree; when off, render legacy.

Exit gate:

- Parity checklist in Section 14 passes.
- No visual regressions for one representative conversation of each type: plain chat, long reasoning, multi-tool run, command approval, screenshot, planning mode, stopped generation, context-length error.

### Phase 4 — Virtualization + Scroll

Files: `src/chat/rendering/VirtualMessageList.tsx`, `ChatViewport.tsx`, `FloatingJumpToBottomButton.tsx`.

- Add `@tanstack/react-virtual` via `bun add @tanstack/react-virtual` (update package.json).
- Replace plain list with virtual list.
- Implement scroll state machine (Section 8).
- Add `content-visibility: auto` to completed message bodies.

Exit gate:

- 1,000-message synthetic conversation scrolls smoothly.
- Streaming at bottom follows smoothly; scrolling away never snaps back.
- Jump-to-latest instant and predictable.
- Phase-1 counters show message-list re-renders near zero during streaming.

### Phase 5 — Streaming Markdown

Files: `src/chat/markdown/*`, updates to `AssistantText.tsx`.

- Implement `PlainStreamingText`, `MarkdownLite`, `MarkdownFull`, `codeBlockLazy.ts`.
- Replace `StreamingMarkdownRenderer` usage in the new tree only.
- Legacy `MarkdownRenderer.tsx` untouched.

Exit gate:

- Streaming markdown parse time per render < 1 ms on typical messages.
- Code blocks > 30 KB render without highlighting but remain copyable.
- No visible layout shift when completed text is promoted from `PlainStreamingText` to `MarkdownLite`/`MarkdownFull`.

### Phase 6 — Composer Split

Files: `src/chat/composer/*`.

- Build new composer from scratch per Section 11.
- Switch new chat tree to the new composer.
- Legacy `CommandCenter.tsx` still used by legacy panel.

Exit gate:

- Every composer capability present in legacy is present in new: mentions, attachments, screenshots, model select, send/stop, mode toggle, planning, queueing, path suggestions.
- Composer does not re-render on streaming deltas.

### Phase 7 — Cutover + Deletion

- Flip `chat.panel.v3` default to `true`.
- Run full parity checklist again.
- Delete:
  - `src/components/ChatPanel.tsx`
  - `src/components/ChatMessage.tsx`
  - `src/hooks/useChatV2.ts`
  - `src/components/MarkdownRenderer.tsx` (confirm no external imports first)
  - `src/components/CommandCenter.tsx`
  - `src/components/ToolCallDisplay.tsx`
  - `src/utils/chatTimeline.ts`
  - `src/utils/messageBlocks.ts`
  - `src/hooks/useChatLegacy.ts` if present
- Remove the `chat.panel.v3` flag and dual-dispatch wiring.
- Remove unused timers, row/activity DOM registries, manual virtualizer helpers.

Exit gate: `bun run build`, `bun test`, `cargo check -p zblade`, `cargo test -p zblade` all green. No references to deleted symbols remain (`grep -R` audit).

## 14. Parity Checklist (used in Phases 3, 6, 7)

Functional:

- Send message; receive streamed response with reasoning, tool calls, and final text.
- Stop generation mid-stream.
- Approve / reject run_command inline in the correct turn.
- Skip tool call.
- Undo last turn.
- Planning mode send + Implement plan action.
- Model selection (local + cloud).
- Queueing: send while streaming, queue visible, edit queued request.
- Screenshot attachment (region + full).
- Image attachments (multi).
- Mentions for symbols, files, commands.
- Path suggestions debounced and cancellable.
- History load (long conversation) with preserved tool calls, approvals, command outputs, images.
- Context-length and message-too-large errors display clearly.
- Todos appear and update.
- Run status dock shows rotating verb while waiting.

Performance (measured via `chatPerf`):

- Message-list renders ≈ 0 per second during streaming of active row.
- Active-row renders ≤ display frame rate.
- Markdown parse time per render < 1 ms while streaming.
- Scroll mode transitions only on genuine user actions.

UX:

- Jump-to-bottom only appears when `detached`.
- No snap-back while user scrolls away.
- Completed tools collapse smoothly without per-row timers.
- Reasoning collapses after completion; active reasoning readable in all themes.

## 15. Testing Strategy

- **Reducer tests**: `src/chat/protocol/chatProtocolReducer.test.ts` covers every action and every AssistantPart transition, including interleaved tool calls and late deltas after `MessageCompleted`.
- **Store tests**: selector stability tests using `useSyncExternalStore` snapshots.
- **Replay tests**: record real event sequences from current runtime (dev-only capture hook added in Phase 1) into JSON fixtures under `src/chat/protocol/__fixtures__/`. Replay into new reducer and assert final state and ordered `parts`.
- **Component tests**: targeted tests for `AssistantText` mode promotion, `ToolCallRow` status rendering, `ApprovalInlineCard` gating, `VirtualMessageList` keying.
- **Perf guards**: add a Node-side unit that rejects regressions in reducer action-cost by wrapping a fixed workload and asserting < threshold (soft gate, warn only).
- **Manual checklist**: Section 14 executed at each cutover gate.

## 16. Risks and Mitigations

- **Selector churn causing extra renders.** Mitigate with reference-stable snapshots; reuse cached arrays when contents are unchanged.
- **Virtualizer measurement thrash with markdown promotion.** Promote only when `status === 'done'`; `MarkdownLite` designed to match `PlainStreamingText` line metrics closely to minimize measurement delta.
- **Dual dispatch doubling event cost in Phase 2.** Keep reducer cheap; bail early when flag is off; unit-measure cost before enabling in dev builds.
- **History migration mismatch.** Gated behind the flag; on any migration error, fall back to legacy panel and log a single structured error event.
- **`@tanstack/react-virtual` sizing inside flex/grid containers.** Use a dedicated scroll container with fixed height; document in `ChatViewport.tsx`.
- **Approval latency.** Approvals resolved via `useChatActions.approveToolCall`; keep using existing Tauri commands to avoid protocol drift.

## 17. Out of Scope (explicit)

- Backend protocol changes (BCP) — separate RFC if needed.
- Removing or redesigning reasoning, tools, approvals, screenshots, queueing, planning mode, model selection.
- Theming rework (themes continue to drive via existing CSS tokens).
- Markdown engine replacement beyond the lite/full split described.
- History persistence changes.

## 18. Definition of Done

- Every acceptance criterion in `docs/2026-05-03-new-chat-panel.md` §"Acceptance Criteria" verified with `chatPerf` counters and manual checklist.
- Legacy files in Section 3 deleted.
- Feature flag removed.
- `bun run build`, `bun test`, `cargo check -p zblade`, `cargo test -p zblade` all green.
- No references to `chatTimeline`, `messageBlocks`, `useChatV2`, `ChatMessage.tsx`, `CommandCenter.tsx`, `ToolCallDisplay.tsx`, `MarkdownRenderer.tsx` remain in the repo.
- `CHANGELOG.md` entry describing the chat panel rewrite.
