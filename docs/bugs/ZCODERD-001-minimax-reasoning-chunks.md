# BUG: zcoderd sends MiniMax M2.5 reasoning as text_chunk instead of reasoning_chunk

**ID:** ZCODERD-001
**Severity:** Medium
**Component:** zcoderd (server-side)
**Reported:** 2026-02-15
**Status:** Resolved (zcoderd server-side fix applied 2026-02-15; client-side workaround reverted)

## Summary

When streaming responses from MiniMax M2.5, zcoderd sends reasoning content (wrapped in `<think>...</think>` tags) as `text_chunk` WebSocket messages instead of `reasoning_chunk` messages. This causes the client to display raw reasoning text inline with the response content, rather than routing it to the collapsible reasoning card UI.

## Expected Behavior

zcoderd should detect `<think>...</think>` blocks in MiniMax M2.5 responses and emit them as `reasoning_chunk` messages, consistent with how it handles other reasoning models (e.g., Kimi K2.x, which correctly sends `reasoning_chunk` messages).

## Actual Behavior

MiniMax M2.5 reasoning arrives as `text_chunk` messages containing raw `<think>` tags. The client receives:

```json
{"type": "text_chunk", "payload": {"content": "<think>\nThe user wants me to..."}}
```

Instead of:

```json
{"type": "reasoning_chunk", "payload": {"content": "The user wants me to..."}}
```

## Impact

- Reasoning text appears inline in the chat message instead of in the reasoning card
- `<think>` / `</think>` tags are visible as raw text to the user
- Inconsistent UX between MiniMax M2.5 and other reasoning models

## Client-Side Workaround (ZaguanBlade)

A defensive fix has been applied in `chat_manager.rs`:

1. `stream_parse_reasoning` is now enabled independently of `stream_plain_text`, so models matching `supports_reasoning_tags()` (which includes `minimax`) always get `<think>` tag parsing.
2. The `flush_batch!` macro now runs the reasoning parser on text chunks when `use_reasoning_parser` is true, even for Zaguan provider streams.

This workaround correctly extracts reasoning from `text_chunk` messages, but the proper fix should be in zcoderd to emit `reasoning_chunk` messages for MiniMax M2.5.

## Affected Models

- MiniMax M2.5 (confirmed)
- Potentially any model where zcoderd forwards `<think>` tags inline rather than as separate reasoning events

## Reproduction

1. Open ZaguanBlade connected to zcoderd
2. Select MiniMax M2.5 model
3. Send any message that triggers reasoning
4. Observe that reasoning text appears inline (without the client workaround) or correctly in the reasoning card (with the workaround)

## Suggested Server Fix

In zcoderd's streaming pipeline for MiniMax M2.5, detect `<think>...</think>` blocks in the model's response and emit them as `reasoning_chunk` messages with the tags stripped, matching the behavior already implemented for Kimi K2.x.
