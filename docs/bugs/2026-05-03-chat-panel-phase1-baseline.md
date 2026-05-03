# Chat Panel Phase 1 Baseline

Date: 2026-05-03
Plan: `docs/2026-05-03-chat-panel-implementation.md`

## Instrumented Counters

Phase 1 adds hidden runtime sampling through `window.__chatPerf`.

Counters sampled once per second:

- `streamEventsReceived`
- `streamFlushes`
- `activeRowRenders`
- `messageListRenders`
- `markdownParseTime.calls`
- `markdownParseTime.ms`
- `virtualizerMeasureCount`
- `scrollModeTransitions`

Gauges:

- `mountedRowCount`

## Static Baseline

Current legacy chat surface size before replacement:

- `src/components/ChatPanel.tsx`: 1,068 lines before instrumentation
- `src/components/ChatMessage.tsx`: 1,025 lines before instrumentation
- `src/hooks/useChatV2.ts`: 1,922 lines before instrumentation
- `src/components/MarkdownRenderer.tsx`: 744 lines before instrumentation
- `src/components/CommandCenter.tsx`: 1,000 lines before instrumentation
- `src/components/ToolCallDisplay.tsx`: 451 lines before instrumentation
- `src/utils/chatTimeline.ts`: 255 lines
- `src/utils/messageBlocks.ts`: 223 lines

Total: 6,688 lines in the legacy chat surface and immediate data-shaping path.

## Runtime Capture

During a dev run, inspect:

```js
window.__chatPerf.samples.slice(-10)
```

Reset with:

```js
window.__chatPerf.reset()
```

The Phase 4 gate will compare these legacy samples against the new virtualized panel. The expected improvement is that `messageListRenders` stays near zero during streaming, while `activeRowRenders` remains capped by display frames.
