# Bug Report: Parallel Output Streams Interleaved in `text_chunk` Messages

## Summary

When using GPT-5.4 (OpenAI Responses API models), zcoderd forwards **two parallel text output streams** interleaved token-by-token in `text_chunk` WebSocket messages, without any field to distinguish which output item each chunk belongs to. This produces garbled, unreadable text in the client.

## Severity

**High** — renders all GPT-5.4 responses unreadable.

## Affected Models

OpenAI models that use the Responses API with parallel output items (confirmed: `openai/gpt-5.4`). Models using the Chat Completions API (Anthropic, older OpenAI) are not affected.

## Observed Behavior

The client receives `text_chunk` messages where two completely different versions of the same response are interleaved at the token level. Example from ZaguanBlade debug log:

```
[ws->rust][text] len=9 preview=" grounded"   ← Stream A: "...grounded,"
[ws->rust][text] len=1 preview=","
[ws->rust][text] len=3 preview="Yes"          ← Stream B starts: "Yes — there are..."
[ws->rust][text] len=7 preview=" calmer"      ← Stream A continues: "calmer, and"
[ws->rust][text] len=1 preview=","
[ws->rust][text] len=4 preview=" and"
[ws->rust][text] len=4 preview=" —"           ← Stream B continues: "— there are"
[ws->rust][text] len=5 preview=" more"        ← Stream A: "more mobile-native"
[ws->rust][text] len=6 preview=" there"       ← Stream B: "there are some"
[ws->rust][text] len=7 preview=" mobile"      ← Stream A
[ws->rust][text] len=4 preview=" are"         ← Stream B
[ws->rust][text] len=7 preview="-native"      ← Stream A
```

When concatenated, this produces:

```
Yes — there are a few clear "AI-slop" signals in the current design, and they're
mostly fixable by making the product feel more grounded,Yes calmer, and — more
there mobile are-native.
```

Instead of the intended:

```
Yes — there are a few clear "AI-slop" signals in the current design, and they're
mostly fixable by making the product feel more grounded, calmer, and more
mobile-native.
```

## Root Cause Analysis

OpenAI's Responses API can produce **multiple output items** in a single response (e.g., `output[0]` and `output[1]`). Each output item has its own `output_index`. When streaming, the API sends `response.output_text.delta` events with an `output_index` field that identifies which output item the delta belongs to.

zcoderd appears to be forwarding deltas from **all output items** as `text_chunk` messages without:

1. Including `output_index` (or equivalent) in the `text_chunk` payload so the client can separate them
2. Filtering to only forward the first/primary output item

The `text_chunk` payload currently only contains `{ "content": "..." }` with no index field.

## Expected Behavior (two options)

### Option A: Include `output_index` in `text_chunk` payloads (preferred)

Add an `output_index` field to `text_chunk` and `reasoning_chunk` messages:

```json
{
  "type": "text_chunk",
  "payload": {
    "content": "Yes",
    "output_index": 0
  }
}
```

This lets the client decide how to handle parallel outputs (display the first, show both side-by-side, etc.). The client can filter by locking onto the first `output_index` it sees.

### Option B: Server-side filtering

Only forward deltas from the primary output item (typically `output_index: 0`) and drop the rest. Simpler, but less flexible.

## Client-Side Mitigation (already implemented in ZaguanBlade)

ZaguanBlade now parses `output_index` from `text_chunk`/`reasoning_chunk` payloads (trying fields `output_index`, `content_index`, `index`) and filters to only accept chunks from the first output index seen per stream. This will work immediately once zcoderd includes the field.

Relevant code: `src-tauri/src/blade_ws_client.rs` (parsing) and `src-tauri/src/chat_manager.rs` (filtering via `accepted_output_index`).

## Evidence

Full debug log available at `ZaguanBlade/debug-output.log`. Key sections:

- **Lines 321–443**: Stream A produces coherent text ("Yes — there are a few clear AI-slop signals..."), then at line 443 Stream B starts a second "Yes" that interleaves with Stream A's continuation.
- **Lines 443–530**: The two streams alternate every 1–3 tokens, producing completely garbled output.
- **Lines 530–6600+**: The interleaving continues for the entire ~6000-token response.

Only a single `chat_request` was sent (line 38), confirming this is not a duplicate request issue.

## Reproduction

1. Send a chat request using `openai/gpt-5.4` that produces a long-form text response (not tool calls)
2. Observe `text_chunk` WebSocket messages
3. After the first ~30 tokens of response text, a second parallel stream begins, and chunks from both streams arrive interleaved

## Questions

1. Is zcoderd using the OpenAI Responses API or Chat Completions API for GPT-5.4?
2. If Responses API: does the OpenAI response include `output_index` on streaming deltas? If so, can zcoderd forward it in `text_chunk` payloads?
3. Is there a reason to forward multiple output items, or should the server filter to only the primary one?
