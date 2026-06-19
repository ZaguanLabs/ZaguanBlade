# Live Vision Model Research - 2026-06-11

## Context

This note updates the two older design docs in `docs/live/`:

- `live-screen-share-33e843.md`
- `live-vision-revised-33e843.md`

Those docs describe a feature where ZaguanBlade locally captures a shared screen/window, keeps a short ring buffer, and sends an on-demand observation burst to zcoderd. zcoderd then asks a vision model to describe the screen in a structured way, injects that description into the worker model, and lets the worker request more observations.

The important requirements are:

1. Server-side orchestration through zcoderd, with provider auth/billing handled by ZaguanCoreX.
2. Screen/window observations from ZaguanBlade, not autonomous remote desktop control.
3. Ability to see recent temporal changes, including sub-second UI flashes and error overlays.
4. Strong OCR for browser error overlays, terminal text, file paths, line numbers, and stack traces.
5. Text output that can be injected into the worker model.
6. Repeatable observation loop: worker can ask zcoderd for another observation.
7. Bounded cost: no continuous cloud streaming while the user is merely sharing a window.

Non-negotiable safety invariant:

Video capture must only start when the user explicitly initiates it from the Command Center Actions menu, e.g. "Share Window" or "Share Screen". It must never start automatically because a chat begins, a worker requests observation, a project opens, a setting is enabled, or a model/tool decides it would be useful. This feature can spend a lot of vision tokens if left running, so the product contract must be user-initiated capture, visible active state, and explicit stop.

## Executive Summary

The old plan should not be implemented as "send 15 FPS directly into Gemini Live." The current Gemini Live API docs still describe video frames as individual JPEG/PNG images with a maximum of 1 frame per second. That is not enough to reliably catch the sub-second glitches called out in the revised architecture.

The best current fit is a hybrid design:

1. User explicitly starts screen/window sharing from the Command Center Actions menu.
2. Keep the ZaguanBlade local high-FPS capture ring buffer while sharing is active.
3. On observation, send a short encoded video clip or selected frame sequence to zcoderd.
4. zcoderd uses a pluggable vision adapter.
5. Since zcoderd already uses ZaguanCoreX, and CoreX supports major providers including Qwen/Alibaba, Qwen3.6 Plus/Flash should be treated as a first-class default candidate rather than an integration-heavy alternative.
6. Gemini video understanding via `generateContent`, likely `gemini-3.5-flash`, should be kept as the strongest Google comparison/fallback path for burst video analysis.
7. Keep Gemini Live as an optional low-latency adapter for "what is on screen now" observations at <=1 FPS.
8. Treat OpenAI, Claude, and Mistral as image/keyframe fallback adapters, not full replacements for temporal video observation.

## Requirements Checklist

| Requirement | Needs model support? | Notes |
| --- | --- | --- |
| Server-side API from zcoderd | Yes | WebSocket or request API both work. Provider auth/billing should be handled by zcoderd/CoreX. |
| Screen/window input | Yes | Usually images or encoded video, not raw desktop stream. |
| User-initiated capture only | Product/UI requirement | Capture starts only from the Command Center Actions menu. No automatic start from chat, worker tools, settings, or project state. |
| Sub-second temporal events | Yes | Needs high-FPS local capture and a backend that can process clips or enough keyframes. |
| Exact OCR | Yes | Dense UI text and terminal output are the make-or-break cases. |
| Structured text report | Yes | Native structured output is nice, but prompt-enforced markdown is enough. |
| On-demand bursts | Mostly zcoderd | This is orchestration, not a model feature. |
| Worker re-observation | Mostly zcoderd | Implement as a tool in the worker model. |
| Continuous low latency | Optional | Helpful, but less important than temporal fidelity for debugging glitches. |

## Updated Provider Findings

### Google Gemini Live API

Current status:

- `gemini-3.1-flash-live-preview` is now the newer Live API model.
- `gemini-2.5-flash-native-audio-preview-12-2025` still exists as Gemini 2.5 Flash Live Preview, but it is no longer the only Live model with video.
- Both Live models support text, images/video, and audio input, with text/audio output.
- Live API supports server-to-server WebSocket integration.
- Gemini Live is still preview.
- The Live API capabilities guide says video frames are sent as individual JPEG or PNG images at max 1 frame per second.

Sources:

- [Gemini Live API overview](https://ai.google.dev/gemini-api/docs/live-api)
- [Gemini Live API capabilities guide](https://ai.google.dev/gemini-api/docs/live-api/capabilities)
- [Gemini 3.1 Flash Live Preview model page](https://ai.google.dev/gemini-api/docs/models/gemini-3.1-flash-live-preview)
- [Gemini 2.5 Flash Live Preview model page](https://ai.google.dev/gemini-api/docs/models/gemini-2.5-flash-native-audio-preview-12-2025)

Fit:

| Requirement | Fit |
| --- | --- |
| Server-side zcoderd session | Good. WebSocket server-to-server is documented. |
| Text output for worker injection | Good. Live models output text and audio. |
| Tool/function support | Good. Function calling is supported. |
| Low-latency current screen state | Good at low visual FPS. |
| Sub-second temporal bugs | Weak if used directly, because Live frame input is documented at max 1 FPS. |
| Cost control | Good if opened only for observations. |

Assessment:

Gemini Live is still useful, but not for the old "15 FPS direct Live stream" idea. It is best for cheap current-state re-observation, voice/video agent use cases, and cases where a 1 FPS screen sample is enough.

If we use it, zcoderd should downsample the observation stream to <=1 FPS and make the prompt explicit:

```text
You are seeing sampled screen frames, not a full-fidelity video. Report visible state and any changes visible between frames. If temporal confidence is low, say so.
```

### Google Gemini Video Understanding via GenerateContent

Current status:

- Gemini can process video files through the File API, Cloud Storage registration, inline data for short clips, and YouTube URLs.
- The docs recommend File API for larger or reusable video files.
- Inline video is suitable for small clips.
- `gemini-3.5-flash` is GA, stable, has a 1M token context window, 65k max output, and is positioned for agentic/coding tasks.
- Gemini video understanding can answer questions about video content and refer to timestamps.
- Media resolution controls exist and token accounting has changed between Gemini 2.5 and Gemini 3 families.

Sources:

- [Gemini video understanding](https://ai.google.dev/gemini-api/docs/video-understanding)
- [What is new in Gemini 3.5 Flash](https://ai.google.dev/gemini-api/docs/whats-new-gemini-3.5)
- [Gemini media resolution](https://ai.google.dev/gemini-api/docs/media-resolution)

Fit:

| Requirement | Fit |
| --- | --- |
| Server-side zcoderd call | Good. Request/response API is easy to orchestrate. |
| Ring-buffer burst | Excellent. Encode last N seconds as a short video clip. |
| Sub-second temporal bugs | Better than Live, because we can send a short high-FPS clip. Needs empirical testing. |
| OCR | Likely strong, especially at 720p or higher. Needs benchmark with real overlays/terminals. |
| Low latency | Medium. Request-based clip upload is slower than Live, but probably acceptable for "user asked a debugging question." |
| Cost control | Good. Only upload clips on demand. |

Assessment:

This is the strongest Google path for the actual revised architecture. It aligns with the ring buffer design: capture locally at 15 FPS, then send the last 5-10 seconds as a compact MP4/WebM observation burst.

Recommended first benchmark:

- 5 second clip, 720p, 10-15 FPS, H.264 or VP9, with an error overlay visible for 200-500 ms.
- Prompt for exact visible text, timestamp range, and temporal sequence.
- Compare `gemini-3.5-flash` against `gemini-3.1-flash-live-preview` sampled at 1 FPS.

### Alibaba Qwen3.6 / Qwen3.5 Vision Models

Current status:

- zcoderd already uses ZaguanCoreX, and ZaguanCoreX supports Qwen/Alibaba alongside the other major providers. That removes most provider-specific implementation cost from the original assessment.
- Alibaba Cloud Model Studio documents `qwen3.6-plus` and `qwen3.6-flash` as image and video understanding models.
- They support text, images, and video input with text output.
- `qwen3.6-plus` and `qwen3.6-flash` are documented with 1M context, up to 2-hour videos, function calling, built-in tools, and structured output.
- Qwen3-VL open model docs emphasize long-context video understanding and timestamp-grounded event localization.

Sources:

- [Alibaba Cloud Model Studio visual understanding models](https://www.alibabacloud.com/help/en/model-studio/vision-model)
- [Alibaba Cloud image and video understanding guide](https://www.alibabacloud.com/help/en/model-studio/vision)
- [Qwen3-VL GitHub repository](https://github.com/QwenLM/Qwen3-VL)

Fit:

| Requirement | Fit |
| --- | --- |
| Server-side zcoderd call | Good. OpenAI-compatible API is documented. |
| Video burst | Excellent on paper. |
| Sub-second temporal bugs | Potentially strong. Needs benchmark. |
| OCR | Strong on paper. Qwen3-VL emphasizes expanded OCR. |
| Structured output | Good. Structured output is documented for Qwen3.6/3.5/Qwen3-VL non-thinking mode. |
| Provider fit | Good for this product because zcoderd already routes through ZaguanCoreX. |

Assessment:

Qwen is likely the best default bet for the first implementation. It is a strong technical match for burst video analysis, and CoreX removes the main implementation objection. The remaining question is not integration cost, but empirical quality: can it reliably extract exact UI/terminal text and detect brief visual events from short screen clips?

### OpenAI

Current status:

- Latest OpenAI frontier models support text and image input with text output.
- `gpt-5.5` is documented as the flagship model for complex reasoning and coding, with 1M context and image input.
- `gpt-realtime-2` supports text, audio, and image input, with text/audio output.
- `gpt-realtime-2` explicitly does not support video.
- Realtime sessions can run up to 60 minutes.
- OpenAI's Realtime docs and release notes frame image input as screenshots/photos added to the conversation, not live video streaming.

Sources:

- [OpenAI models overview](https://developers.openai.com/api/docs/models)
- [OpenAI images and vision guide](https://developers.openai.com/api/docs/guides/images-vision)
- [OpenAI realtime conversations guide](https://developers.openai.com/api/docs/guides/realtime-conversations)
- [GPT-Realtime-2 model page](https://developers.openai.com/api/docs/models/gpt-realtime-2)
- [OpenAI gpt-realtime release note](https://openai.com/index/introducing-gpt-realtime/)

Fit:

| Requirement | Fit |
| --- | --- |
| Server-side zcoderd call | Good. |
| Image/keyframe observations | Good. |
| Video burst | Weak. Realtime model explicitly says video is not supported. |
| Sub-second temporal bugs | Weak unless zcoderd pre-selects enough frames around the event. |
| OCR | Likely strong on screenshots. Needs benchmark. |
| Worker model integration | Excellent if OpenAI is already used for the worker. |

Assessment:

OpenAI is a good fallback if the observation payload is selected screenshots or keyframes. It is not a full match for the video-burst requirement unless zcoderd implements a preprocessing layer that turns local video into a small sequence of high-signal frames.

Potential OpenAI adapter shape:

1. Detect changed frames locally or in zcoderd.
2. Pick 8-20 keyframes around a user request.
3. Send them to `gpt-5.5` or `gpt-realtime-2` with timestamps in the prompt.
4. Ask for a structured report and explicit confidence on temporal claims.

### Anthropic Claude

Current status:

- Anthropic docs say all current Claude models support text and image input, text output, multilingual capabilities, and vision.
- Claude Fable 5 is documented as Anthropic's most capable widely released model, with 1M context and 128k max output.
- Claude Opus 4.8 is recommended for complex reasoning, long-horizon agentic coding, and high-autonomy work.
- Claude Opus 4.7 introduced higher-resolution image support, useful for screenshot understanding and computer use.
- The vision guide is image-oriented. I did not find a first-class live video API in Anthropic's public docs.

Sources:

- [Claude models overview](https://platform.claude.com/docs/en/about-claude/models/overview)
- [Claude vision guide](https://platform.claude.com/docs/en/build-with-claude/vision)
- [Claude migration guide, high-resolution image support](https://platform.claude.com/docs/en/about-claude/models/migration-guide)

Fit:

| Requirement | Fit |
| --- | --- |
| Server-side zcoderd call | Good. |
| Image/keyframe observations | Good. |
| Video burst | Weak. Public docs are image-based. |
| Sub-second temporal bugs | Weak unless represented by selected frames. |
| OCR | Likely strong for screenshots, especially high-resolution image paths. |
| Coding worker context | Excellent. Claude is strong for coding/debugging if also used as worker. |

Assessment:

Claude is a strong screenshot/keyframe adapter. It is not the default fit for raw video or live screen streams. It could be very useful as a second-stage verifier: Gemini/Qwen produces a temporal report, Claude worker reasons over the code and report.

### Mistral

Current status:

- Mistral docs list vision-capable models through Chat Completions, including `mistral-large-2512`, `mistral-medium-2508`, `mistral-small-2506`, and Ministral 3 models.
- The documented vision path is image input, not live video.
- Mistral's main advantage is openness/deployment flexibility, especially if self-hosting or European provider posture matters.

Sources:

- [Mistral vision docs](https://docs.mistral.ai/studio-api/conversations/vision)
- [Mistral models overview](https://docs.mistral.ai/models/overview)

Fit:

| Requirement | Fit |
| --- | --- |
| Server-side zcoderd call | Good. |
| Image/keyframe observations | Good. |
| Video burst | Weak in the documented hosted API path. |
| Sub-second temporal bugs | Weak unless represented by selected frames. |
| OCR | Needs benchmark. |
| Self-host option | Better than closed providers. |

Assessment:

Mistral is a fallback or future self-host path, not the best first implementation for live vision debugging.

## Model Shortlist

| Rank | Model/API | Use it for | Why |
| --- | --- | --- | --- |
| 1 | `qwen3.6-plus` / `qwen3.6-flash` via CoreX | Default burst video candidate | Strong documented video support, structured output, 1M context, and no extra provider implementation cost because zcoderd already uses CoreX. |
| 2 | `gemini-3.5-flash` via Gemini video understanding | Google burst video fallback/comparison | Stable, 1M context, video input, good Google ecosystem fit. |
| 3 | `gemini-3.1-flash-live-preview` via Gemini Live | Optional low-latency current-state observation | Newer Live model, server-to-server WS works, but video frame rate cap makes it poor for sub-second glitches. |
| 4 | `gpt-5.5` / `gpt-realtime-2` | Screenshot or keyframe fallback | Strong OCR/reasoning likely, but no realtime video support in `gpt-realtime-2`. |
| 5 | `claude-fable-5` / `claude-opus-4-8` | Screenshot or keyframe fallback | Strong vision and coding reasoning, but public docs are image-oriented. |
| 6 | `mistral-large-2512` | Open/provider-diverse image fallback | Vision-capable, but not a strong video/live fit from current docs. |

## Architecture Recommendation

Keep the revised doc's reactive model, but change the media contract:

```text
User explicitly clicks Share Window or Share Screen in the Command Center Actions menu
  -> ZaguanBlade captures locally at 15 FPS into a ring buffer
  -> UI shows an always-visible active sharing state and Stop Sharing control
  -> No cloud frames sent while idle

User asks a debugging question
  -> ZaguanBlade sends an observation burst to zcoderd:
     - Prefer: short video clip, e.g. last 5-10 seconds
     - Fallback: selected timestamped keyframes
  -> zcoderd calls a vision adapter
  -> Adapter returns structured observation report
  -> zcoderd injects report into worker model
  -> Worker can call request_screen_observation again
```

Worker-initiated re-observation is allowed only after the user has already started sharing. If sharing is inactive, `request_screen_observation` must fail with a clear "screen sharing is not active" tool result and must not start capture.

Do not make the Blade WS protocol hard-code Gemini Live semantics. It should carry generic observation media:

```json
{
  "type": "screen_observation",
  "payload": {
    "session_id": "...",
    "observation_id": "...",
    "target_title": "Firefox - localhost:3000/dashboard",
    "prompt": "Debug this hydration issue",
    "media": {
      "kind": "video_clip",
      "mime_type": "video/mp4",
      "width": 1280,
      "height": 720,
      "fps": 15,
      "duration_ms": 5000,
      "data": "<base64>"
    }
  }
}
```

For providers that only support images, zcoderd can transform this into:

```json
{
  "kind": "timestamped_frames",
  "frames": [
    { "t_ms": 0, "mime_type": "image/jpeg", "data": "..." },
    { "t_ms": 250, "mime_type": "image/jpeg", "data": "..." },
    { "t_ms": 500, "mime_type": "image/jpeg", "data": "..." }
  ]
}
```

## Capture Dependency: scap Development Path

The model/provider decision does not remove the hardest local dependency: ZaguanBlade still needs reliable continuous screen/window capture.

The current workspace has a local `scap` clone at `streaming/scap`, on `main` at `c03f15a` (`fix windows build`), with `origin` pointing at `https://github.com/ZaguanLabs/scap`. Inspecting that clone confirms the older plan's core concern:

- Linux capture is still PipeWire/xdg-desktop-portal based (`src/capturer/engine/linux/portal.rs`).
- There is no X11 backend under `src/capturer/engine/linux/`.
- Linux target enumeration still returns an empty list (`src/targets/linux/mod.rs`).
- Linux `get_output_frame_size` still returns `[0, 0]`.
- Linux capture state is still process-global through `CAPTURER_STATE`.

For this feature, `scap` should be treated as a real subproject with an upstreamable patch plan, not as a quick app-local hack.

### Recommended branch model

```text
ZaguanLabs/scap
  main                  tracks the current fork/upstream base
  zaguan                integration branch consumed by ZaguanBlade
  feat/linux-x11-xshm   upstreamable X11 backend
  feat/linux-bgra       upstreamable Linux pixel-format negotiation/fixes
  feat/linux-state      upstreamable per-instance state cleanup
  feat/linux-targets    upstreamable Linux target/dimension improvements where possible
```

Use `streaming/scap` as the development clone while researching and patching. Once the patches are usable, push them to `ZaguanLabs/scap` and consume either:

```toml
scap = { git = "https://github.com/ZaguanLabs/scap", branch = "zaguan" }
```

or, for local iteration only:

```toml
scap = { path = "../streaming/scap" }
```

Prefer keeping any local path override in developer-only config rather than in committed release/CI manifests.

### Upstreamable work

These changes should be kept general and suitable for PRs back to the upstream `scap` project:

1. **Linux X11 backend via XShm**
   - Add `src/capturer/engine/linux/x11.rs`.
   - Detect X11 vs Wayland at runtime using `$XDG_SESSION_TYPE`, `$WAYLAND_DISPLAY`, and `$DISPLAY`.
   - Use XShm for timer-based capture at configured FPS.
   - Optionally add XDamage later to skip unchanged frames.
   - Keep the API generic: no ZaguanBlade WS messages, ring buffers, or model-specific frame processing.

2. **Linux engine selection**
   - Route Wayland/session-with-portal through the existing PipeWire backend.
   - Route X11 sessions without portal ScreenCast support through the XShm backend.
   - Return clear errors when neither path is available.

3. **Linux pixel-format handling**
   - Add/verify BGRA/BGRx/RGBx negotiation and conversion paths.
   - Make the frame type returned by Linux predictable enough that app code does not need Linux-specific guesswork.

4. **Per-instance capture state**
   - Replace process-global `CAPTURER_STATE` with capturer-owned state.
   - This is not required for the first ZaguanBlade feature if only one screen share can run, but it is an upstream-quality cleanup and reduces surprising behavior.

5. **Linux target/dimension support**
   - X11 display enumeration is upstreamable.
   - X11 window enumeration is likely upstreamable if implemented through normal X11 APIs.
   - Wayland target selection should remain portal-driven, because compositors intentionally mediate this.
   - `get_output_frame_size` should return useful dimensions on Linux where possible.

6. **Error handling and examples**
   - Convert panics and vague portal failures into actionable errors.
   - Add an example that captures frames at a requested FPS and reports frame size, pixel format, and dropped frames.

### Zaguan-specific work

These pieces should stay in ZaguanBlade/zcoderd, not in `scap`:

- Local ring buffer policy.
- MP4/WebM/JPEG encoding for model observation payloads.
- Frame sampling/keyframe selection for image-only providers.
- Blade WebSocket protocol.
- zcoderd/CoreX provider selection.
- Vision prompts and normalized observation reports.
- UI state, settings, progress messages, and cost/billing behavior.
- User consent and command-center-only capture start policy.
- Any heuristic tuned specifically for "debug this UI" observations.

The rule is simple: if it helps any Rust app capture the screen, push it toward `scap`; if it helps ZaguanBlade turn captures into AI debugging context, keep it in ZaguanBlade/zcoderd.

### Validation milestones

Before building the full ZaguanBlade UI, validate the capture stack by itself:

1. On Mageia/Openbox X11, capture the active display for 60 seconds at 15 FPS.
2. Confirm CPU usage, frame timing, and dropped-frame behavior.
3. Confirm output is usable at 720p and 1080p for terminal/browser OCR.
4. Confirm stop/start works repeatedly in one process.
5. Confirm Wayland portal capture still works on a compositor with ScreenCast support.
6. Run `cargo fmt`, `cargo test`, and a Linux `cargo check` in `streaming/scap`.
7. Once Linux changes are isolated, smoke-test macOS and Windows via CI or contributors before sending upstream PRs.

This means the practical first implementation order is:

```text
scap Linux capture spike -> ZaguanBlade ring buffer -> zcoderd/CoreX Qwen observation -> UI polish
```

## Suggested zcoderd Adapter Interface

```ts
interface VisionObservationRequest {
  observationId: string;
  sessionId: string;
  targetTitle?: string;
  userPrompt: string;
  focus?: string;
  media:
    | VideoClip
    | TimestampedFrameSequence;
}

interface VisionObservationReport {
  provider: string;
  model: string;
  observationId: string;
  confidence: "high" | "medium" | "low";
  screenState: string[];
  visibleErrors: string[];
  terminal: string[];
  temporalEvents: string[];
  changesSinceLastObservation: string[];
  rawText: string;
}
```

The worker model should receive the normalized report, not provider-specific output.

## Vision Prompt Update

Use a stricter prompt than the older docs:

```text
You are a screen observer for a software developer.

Your output is consumed by another coding model. Be exact and structured.

Report only what is visible in the provided screen media. If the media is a video or timestamped frames, include timing estimates for events.

Output:
SCREEN STATE:
- Window:
- URL or app route:
- UI state:

VISIBLE ERRORS:
- Quote exact visible error text, stack traces, file paths, and line numbers.

TERMINAL:
- Quote relevant terminal output exactly.

TEMPORAL EVENTS:
- Include things that appeared, disappeared, flickered, loaded, or changed.
- Include approximate timestamps or frame ranges when possible.

CHANGES SINCE LAST OBSERVATION:
- Report meaningful changes only.

CONFIDENCE:
- high, medium, or low.
- If the frame rate or resolution is insufficient, say what could have been missed.
```

## Implementation Consequences for ZaguanBlade

The old capture work still mostly stands:

- `ZaguanLabs/scap` still makes sense for continuous local capture, with `streaming/scap` as the current development clone.
- The X11 backend issue remains independent of model choice.
- Keep the local ring buffer.
- Add clip encoding, not just JPEG-per-frame transport.
- The UI should still say "Share Window" / "Stop Sharing"; it should not expose provider details unless there is a settings page.

Changes from old docs:

- Default cloud observation should be a burst clip or keyframe set, not a continuous Live session.
- The FPS selector controls local capture fidelity, not necessarily provider ingest FPS.
- Keep Blade provider-agnostic. Provider choice should live in zcoderd/CoreX unless there is a strong reason to expose it in Blade settings.
- Avoid adding Google-specific settings to Blade if zcoderd/CoreX is the provider boundary.
- Sharing may be enabled in settings, but enabling a setting must not start capture. Capture starts only through the Command Center Actions menu.
- Worker/model re-observation must not auto-start capture. It can only consume an already active user-initiated share.

## Risks and Unknowns

1. Gemini video understanding may not preserve every 15 FPS frame internally. It must be benchmarked with real sub-second UI flashes.
2. Provider upload latency for short clips may be noticeable. Need measure end-to-end observation time.
3. Dense terminal text may require 1080p or crop-aware capture instead of 720p.
4. Base64 video clips over Blade WS may be too heavy for long bursts. Prefer binary WS frames or zcoderd upload handoff if this becomes a bottleneck.
5. Provider-specific auth and billing should stay behind CoreX; Blade should not grow one settings field per provider.
6. Qwen's documented fit is strong, but it still needs a real screen-debugging benchmark before committing.
7. `scap` Linux/X11 support is still a real development dependency. The app feature should not be started as a UI-first task until capture is proven on the target dev environment.
8. Accidental automatic capture would be a serious product bug because it can burn tokens and capture sensitive screen contents. Guard this in UI state, Tauri commands, and zcoderd tool handling.

## Recommended Next Step

Before implementing the full UI, run two parallel validation tracks.

Capture track:

1. Use `streaming/scap` to build the Linux X11/XShm capture spike.
2. Prove 15 FPS capture on Mageia/Openbox X11 for at least 60 seconds.
3. Confirm stop/start behavior, frame size, pixel format, CPU use, and dropped frames.
4. Split generic patches into upstreamable branches before wiring them into ZaguanBlade.

Model track:

1. Record 10 short screen clips:
   - React hydration overlay
   - Terminal error
   - Brief i18n key flash
   - Loading spinner flash
   - Browser console visible
   - Dense stack trace
2. Run each through:
   - `qwen3.6-flash` video clip through CoreX
   - `qwen3.6-plus` video clip through CoreX if latency/cost is acceptable
   - `gemini-3.5-flash` video clip
   - `gemini-3.1-flash-live-preview` sampled at 1 FPS
   - `gpt-5.5` keyframes
   - `claude-opus-4-8` or `claude-fable-5` keyframes
3. Score:
   - Exact text extraction
   - File/line extraction
   - Temporal event detection
   - False positives
   - Latency
   - Cost

If the capture spike works and Qwen handles the clips well, implement the first production path through CoreX:

```text
ZaguanBlade ring buffer -> short video clip -> zcoderd/CoreX Qwen adapter -> normalized report -> worker model
```

Keep Live API support behind the adapter interface for future low-latency observation, but do not make it the core feature until the 1 FPS limitation no longer matters for the debugging use case.
