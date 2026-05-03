# New Chat Panel Design

Date: 2026-05-03

## Executive Summary

The current chat panel is over-engineered in the wrong places. It tries to compensate for expensive rendering with custom virtualization, scroll heuristics, memo comparators, block reconstruction, streaming markdown segmentation, row-height estimation, jump registries, and multiple derived timelines. Those mechanisms now cost enough CPU that they have become part of the problem.

The replacement should be simpler:

1. The chat store owns protocol state and creates immutable, normalized turns.
2. The viewport owns scrolling and visibility, using browser primitives and one proven virtual list library.
3. Message rows render data they are handed; they do not infer timeline structure.
4. Streaming updates only the active assistant row at a fixed frame budget.
5. Markdown parsing is avoided while text is actively streaming except for stable completed chunks.
6. Tool activity, approvals, task state, queue state, and composer state are separate UI islands.

The goal is not fewer features. The goal is fewer cross-cutting features.

## Current Shape

The hot files are:

- `src/components/ChatPanel.tsx` - 1,068 lines
- `src/components/ChatMessage.tsx` - 1,025 lines
- `src/hooks/useChatV2.ts` - 1,922 lines
- `src/components/MarkdownRenderer.tsx` - 744 lines
- `src/components/CommandCenter.tsx` - 1,000 lines
- `src/components/ToolCallDisplay.tsx` - 451 lines
- `src/utils/chatTimeline.ts` - 255 lines
- `src/utils/messageBlocks.ts` - 223 lines

That is 6,688 lines for the visible chat surface and its immediate data shaping. The size alone is not the issue; the issue is that those lines overlap in responsibility.

## Main Problems

### 1. The Panel Does Data Modeling

`ChatPanel.tsx` derives chat rows, owns virtual range state, manages row element registries, stores activity target registries, computes jump keys, decides which rows are active, estimates row heights, handles scroll anchoring, switches history/chat tabs, owns task panel collapse state, owns queue edit prefill state, and renders the composer.

A panel should compose layout. This panel is also a timeline engine, virtualizer, scroll controller, navigation service, and status bar coordinator.

### 2. The Hook Does UI Normalization

`useChatV2.ts` listens to backend events, buffers out-of-order deltas, accumulates content, reconstructs block arrays, normalizes content before/after tools, inserts assistant messages, mutates tool status, maps command execution output into message blocks, manages timers, owns active todos, owns queue state, owns selected model state, requests editor context, and exposes React state.

Some of that belongs in an event reducer. Some belongs near backend protocol mapping. Some belongs in small action hooks. It should not all live in one React hook that re-renders a large panel.

### 3. Streaming Invalidates Too Much React

Streaming updates append text, rebuild block arrays, queue updates, flush on `requestAnimationFrame`, update `state.messages`, rebuild ID maps in effects, rerun `deriveChatRows`, recompute virtualized rows, recompute row heights, and re-render the active message. The implementation tries to memoize many of those steps, but the basic pipeline still routes every token through top-level React state.

The active assistant text is the only thing changing on most streaming frames. The rest of the panel should not care.

### 4. Custom Virtualization Is Fragile

The custom virtualizer estimates heights with text length, viewport width, image count, tool count, command output count, reasoning count, mentions, plan summary, and pending approval state. It then slices only the older rows while keeping a tail unvirtualized.

This has three costs:

- CPU cost from row derivation, height estimation, offsets, range calculation, and scroll throttling.
- Correctness risk because estimated heights diverge from real markdown/tool output heights.
- Maintenance cost because every new row feature must update the estimator.

Use a virtualizer that measures real elements.

### 5. Markdown Is Too Expensive While Streaming

`StreamingMarkdownRenderer` incrementally segments markdown, parses stable blocks, handles live fences, special-cases lists and tables, and still renders through `ReactMarkdown` for much of the output. Code blocks use `react-syntax-highlighter`, which is expensive for large or frequently changing blocks.

During streaming, correctness should mean "readable and stable", not "fully highlighted and perfectly parsed every frame".

### 6. Memo Comparators Hide Design Issues

`ChatMessage` has a custom comparator that checks message content, reasoning, streaming sequence, mentions, tool call length, block length, pending actions, pending approvals, and callback presence. `CommandCenter` has another custom comparator. These are signs that the component tree receives too much state.

A good chat panel should need very little custom equality logic because each component subscribes only to the state it uses.

### 7. Timers and Animation Frames Are Scattered

There are timers/frames for:

- streaming flushes
- completion cleanup
- tool activity throttling
- tool activity clearing
- todo summary delay
- message count scroll
- conversation load scroll
- streaming scroll
- viewport metric update
- visible range update
- virtualization scroll throttling
- jump highlighting
- reasoning autoscroll
- textarea resize
- copy-button reset
- tool completion fade

Some are legitimate. Together they make behavior hard to reason about and can keep the UI busy when nothing important is happening.

## Design Principles

Every line in the replacement should pass one of these tests:

- Does this line preserve protocol correctness?
- Does this line render visible information?
- Does this line handle direct user input?
- Does this line prevent measurable work?
- Does this line isolate a known expensive operation?

If not, remove it.

## Proposed Architecture

### Component Tree

```txt
ChatPanel
  ChatHeader
  ChatViewport
    VirtualMessageList
      MessageRow
        UserMessage
        AssistantTurn
          ReasoningSection
          AssistantText
          ToolTimeline
          CommandResult
          ApprovalCard
        SystemMessage
  FloatingJumpToBottomButton
  RunStatusDock
  TaskStrip
  QueueStrip
  Composer
```

`ChatPanel` should not know about block reconstruction, row offsets, tool-call insertion, or command output ordering. It passes IDs and callbacks.

### Data Ownership

Use one chat store with normalized entities:

```ts
interface ChatStoreState {
  conversationId: string | null;
  messageOrder: MessageId[];
  messagesById: Record<MessageId, ChatMessageEntity>;
  activeTurnId: string | null;
  pendingApprovalByToolCallId: Record<string, ApprovalEntity>;
  commandResultByToolCallId: Record<string, CommandResultEntity>;
  toolCallById: Record<string, ToolCallEntity>;
  activeTodos: TodoItem[];
  queuedRequests: QueuedRequest[];
  runStatus: RunStatus;
}
```

Render rows become a cheap selector:

```ts
type ChatRow =
  | { type: 'message'; id: MessageId }
  | { type: 'date-separator'; day: string };
```

Do not store `blocks` as the primary source of truth. Blocks are a render projection for assistant messages. The reducer should preserve event order with a simple `parts: AssistantPart[]` array:

```ts
type AssistantPart =
  | { type: 'reasoning'; id: string; text: string; status: 'streaming' | 'done' }
  | { type: 'text'; id: string; text: string; status: 'streaming' | 'done' }
  | { type: 'tool_call'; id: string }
  | { type: 'command_result'; toolCallId: string };
```

This removes `content_before_tools`, `content_after_tools`, `normalizeSplitBlocks`, `moveExistingContentAfterTools`, and most `messageBlocks.ts` logic from the live path. Legacy history can be migrated once at load time into `parts`.

### Event Pipeline

Split the current `useChatV2.ts` into four modules:

- `chatProtocolReducer.ts` - pure reducer from backend events to normalized state.
- `chatEventBridge.ts` - Tauri listeners and event decoding only.
- `useChatActions.ts` - send, stop, approve, skip, undo, model selection.
- `useChatStore.ts` - subscription boundary for React.

Backend events should enter through one queue:

```txt
Tauri event -> decode -> reducer action -> store mutation -> subscribed components update
```

No React component should assemble message text from protocol deltas.

### Streaming Budget

Streaming should update the UI at most once per animation frame and only for the active assistant part.

Rules:

- Append incoming text to mutable buffers outside React.
- Flush to the store at most every `requestAnimationFrame`.
- If the tab/window is hidden, flush at 250 ms.
- Do not rebuild all messages on flush.
- Do not recompute row arrays on text delta.
- Do not run syntax highlighting on text delta.

The active row can subscribe to `activeTextVersion` or directly to its message entity. The message list should subscribe to `messageOrder`, which changes only when messages are added/removed/reordered.

### Virtualization

Replace the custom virtualizer with `@tanstack/react-virtual`.

Why:

- It measures real row heights.
- It handles variable height rows.
- It supports overscan.
- It avoids maintaining a local row-height estimator.
- It removes `computeVisibleVirtualRange`, `estimateChatRowHeight`, `virtualizedRowOffsets`, `virtualizedRowHeights`, spacer math, viewport metric state, and scroll throttling code.

Policy:

- Virtualize all completed rows.
- Keep the active assistant row mounted.
- Use `content-visibility: auto` on completed message bodies as an extra browser-level guard.
- Use stable row keys from message IDs.

### Scroll Behavior

Use one scroll state machine:

```ts
type ScrollMode = 'following' | 'detached';
```

Rules:

- Start in `following`.
- User scrolls away from bottom: switch to `detached`.
- User clicks jump button or scrolls to bottom: switch to `following`.
- New user message: force `following`.
- Streaming delta: if `following`, keep bottom anchored; if `detached`, do nothing.

Use an `IntersectionObserver` bottom sentinel or virtualizer end state, not both. Avoid smooth scrolling during streaming. Smooth scrolling is acceptable only for explicit user jumps.

### Markdown Rendering

Use three rendering modes:

1. `PlainStreamingText` for the active unfinished tail.
2. `MarkdownLite` for completed assistant text without code fences.
3. `MarkdownFull` for completed text with code fences or tables.

Implementation:

- While streaming, render the live tail as escaped plain text in a `<pre>`.
- Promote completed paragraphs to markdown only when a blank line closes the paragraph or the message completes.
- Use plain `<pre><code>` while streaming inside a code fence.
- Load `react-syntax-highlighter` lazily and only for completed, visible code blocks.
- Cap syntax highlighting for large blocks. For example, over 30 KB or 800 lines, render plain code with copy.

This removes most of the current streaming markdown complexity and keeps the UI readable under load.

### Tool Timeline

Tool rendering should not parse arguments on every render. Parse once in the reducer when a tool call becomes complete enough to display.

Store:

```ts
interface ToolCallEntity {
  id: string;
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
```

`ToolCallDisplay` should render this display model. It should not own friendly-name maps, JSON parsing, path extraction, range extraction, status mapping, and result shaping.

### Approvals

Approvals should be their own store slice keyed by `toolCallId`. Message rows should ask:

```ts
const approval = useApprovalForToolCall(toolCallId);
```

This removes panel-level searching for approval target rows and avoids attaching pending action arrays to derived rows.

For jump-to-approval, store `activeApprovalToolCallId`. The viewport can ask the virtualizer to scroll to the row containing that tool call. No DOM registry is needed for every row and every activity item.

### Composer

Split `CommandCenter.tsx` into:

- `Composer`
- `ComposerTextarea`
- `ComposerToolbar`
- `ModelSelectButton`
- `MentionSuggestions`
- `AttachmentControls`
- `ScreenshotPickerLauncher`

The textarea should own only text, selection, and height. Screenshot capture, file upload, model availability checks, and path suggestion fetching should live in small hooks.

Path suggestions should use a debounced external value and abort stale requests. The visible composer should not re-render because a streaming message changed.

### Visual Design

Keep the UI dense and work-focused. The chat panel is an operational tool, not a landing page.

Recommended styling:

- Full-height panel with a quiet header and fixed composer.
- Message rows use flat surfaces with subtle borders; no heavy shadows per row.
- User messages are compact and right-aligned only if that improves scanning; otherwise keep one column for code-heavy conversations.
- Assistant rows use a narrow left rail with icon/status and a main content column.
- Tool calls render as a compact vertical timeline, collapsed by default after completion.
- Command output renders in a monospace block with line clamp and explicit expand.
- Reasoning is collapsed by default once complete, open while active only if it has recent content.
- The active run status is a small dock above the composer, not a second competing timeline.

Avoid animated spinners on many rows. Only the active operation should animate.

## Code to Delete or Replace

Delete from the live path:

- `computeVisibleVirtualRange`
- `estimateChatRowHeight`
- `findFirstUnvirtualizedChatRowIndex`
- manual virtual spacer code
- row and activity DOM registries
- pending row jump state
- `normalizeSplitBlocks`
- `content_before_tools` / `content_after_tools` inference during streaming
- streaming markdown table/list special casing
- tool argument parsing inside `ToolCallDisplay`
- `Date.now()` calculations inside `ChatMessage` render
- custom `ChatMessage` comparator, after row subscriptions are narrow

Keep or adapt:

- `MessageBuffer` sequence handling, but move it below React.
- Image preview reconciliation, but isolate it to history loading.
- `ensureMessagesHaveBlocks`, but only as a legacy migration helper.
- `recordDebugPerf`, but add timing and row-specific counters.

## Migration Plan

### Phase 1: Instrument Before Replacing

Add these counters before touching behavior:

- stream events received per second
- stream flushes per second
- active row renders per second
- message list renders per second
- markdown parse time per render
- virtualizer measure count
- scroll mode transitions
- number of mounted rows

Acceptance target: during a long streamed response, only the active row and run status should update continuously.

### Phase 2: Introduce Normalized Store

Create the new reducer and selectors alongside `useChatV2`. Feed the same Tauri events into it behind a feature flag.

Do not change visuals yet. Verify that the normalized state can reconstruct the current conversation, including:

- streamed text
- streamed reasoning
- interleaved tool calls
- command results
- approvals
- skipped tools
- stopped generation
- context-length errors
- message-too-large errors
- todo updates
- loaded history

### Phase 3: Replace Message Rendering

Build `MessageRow`, `AssistantTurn`, `AssistantText`, and `ToolTimeline` against the normalized store. Keep the existing `CommandCenter` temporarily.

Remove block inference from rendering. The renderer receives ordered `parts`.

### Phase 4: Replace Virtualization and Scroll

Install `@tanstack/react-virtual` and replace manual virtual range code. Implement the two-state scroll model.

Acceptance target:

- 1,000 message history remains scrollable.
- Streaming at bottom follows smoothly.
- Scrolling away never snaps back.
- Jump-to-latest is instant and predictable.

### Phase 5: Replace Streaming Markdown

Introduce `PlainStreamingText`, `MarkdownLite`, and lazy `MarkdownFull`.

Acceptance target:

- Active streaming text does not run full markdown parse every frame.
- Completed messages still render markdown correctly.
- Large code blocks do not freeze the UI.

### Phase 6: Split Composer

Break `CommandCenter` into smaller components and hooks. This should be low risk after the viewport is stable.

### Phase 7: Remove Legacy Paths

After parity is confirmed:

- remove manual virtualizer
- remove old block normalization live path
- remove old memo comparators
- remove unused timers
- remove old row derivation helpers

## Acceptance Criteria

The new chat panel is acceptable when:

- While streaming, message list renders stay near zero after the active assistant row is mounted.
- Active assistant row renders are capped to animation frames, not token count.
- CPU use remains low during long responses with tools and code blocks.
- The panel handles at least 1,000 historical messages without custom height estimates.
- The user can scroll away during streaming without being pulled back.
- The jump-to-latest button appears only when detached.
- Tool approvals appear in the correct turn without searching all messages on every render.
- Completed tool calls fade visually without per-row timers.
- Large user messages render as plain text without markdown parsing.
- Large code blocks render without syntax highlighting by default.
- The composer remains responsive while the assistant is streaming.

## Proposed File Layout

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
    ChatViewport.tsx
    MessageRow.tsx
    AssistantTurn.tsx
    AssistantText.tsx
    ToolTimeline.tsx
    ToolCallRow.tsx
    ApprovalInlineCard.tsx
    SystemMessage.tsx
  composer/
    Composer.tsx
    ComposerTextarea.tsx
    ComposerToolbar.tsx
    MentionSuggestions.tsx
    useComposerAttachments.ts
    usePathSuggestions.ts
  markdown/
    PlainStreamingText.tsx
    MarkdownLite.tsx
    MarkdownFull.tsx
```

## Non-Goals

- Do not redesign the backend protocol in this pass.
- Do not remove history support.
- Do not remove reasoning, tools, approvals, screenshots, queueing, planning mode, or model selection.
- Do not chase micro-optimizations before ownership boundaries are fixed.

## Final Recommendation

Rewrite the chat panel around a normalized event store and measured virtualization. Do not incrementally add more memoization or scroll heuristics to the current implementation. The current code already proves that cleverness has exceeded the complexity budget.

The new version should be boring in the best way: one event reducer, one virtual list, one scroll state machine, one active streaming row, one composer, and small render components that display already-normalized data.
