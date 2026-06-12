# Live Vision — Revised Architecture

Revised plan for the live screen sharing feature, correcting the model name, redefining the vision session as ephemeral/on-demand bursts orchestrated by zcoderd, and detailing how the vision model and worker model communicate to actually solve bugs.

## What Changed From the Original Plan

| Aspect | Original | Revised |
|--------|----------|---------|
| **Gemini model** | `gemini-2.0-flash-live-001` (doesn't exist) | `gemini-2.5-flash-native-audio-preview-12-2025` (only current Live API model with video) |
| **Session lifetime** | Persistent — open for entire sharing duration | **Ephemeral** — open for seconds, closed once vision model reports, re-opened on demand |
| **2-min session limit** | Problem requiring GoAway workaround | Non-issue — sessions are seconds long |
| **Frame streaming** | Continuous 15-30 FPS for entire session | **Burst** — stream frames during active observation window only |
| **Two-model comm** | Vague "inject into context" | Defined orchestration: vision reports → zcoderd routes to worker → worker can request re-observation |
| **Monetization** | Not addressed | BYOK initially; zaguancorex billing TBD |
| **User control** | Start/stop only | Start/stop + progress messages in chat showing what's happening |

## Architecture: The Observation Loop

```
User clicks [Share Window] → picks browser window
    │
    ▼
┌─ ZaguanBlade ──────────────────────────────────────────────┐
│  scap starts capturing at 15 FPS into a RING BUFFER       │
│  Frames → resize 720p → JPEG q75 → base64 → buffer       │
│  (NO frames sent to zcoderd yet — just buffering locally)  │
│  Status: "Screen sharing active" indicator in UI           │
└────────────────────────────────────────────────────────────┘

... time passes, user sees a glitch ...

User sends message: "Debug this hydration issue"
    │
    ▼
┌─ ZaguanBlade ──────────────────────────────────────────────┐
│  1. Flush ring buffer: send last N seconds of frames       │
│  2. Send screen_share_start + buffered frames via Blade WS │
│  3. Continue sending live frames until told to stop        │
│  4. Show progress: "Observing window..."                   │
└────────────────────┬───────────────────────────────────────┘
                     │ Blade WebSocket
                     ▼
┌─ zcoderd ──────────────────────────────────────────────────┐
│                                                            │
│  ┌─ Vision Session (ephemeral) ─────────────────────────┐  │
│  │  Open Gemini Live WS → send setup                    │  │
│  │  Stream buffered frames (recent history)             │  │
│  │  + live frames for a few more seconds                │  │
│  │  Gemini returns TEXT description                     │  │
│  │  → CLOSE session (or pause frame sending)            │  │
│  └──────────────────────────┬───────────────────────────┘  │
│                             │                              │
│  ┌─ Orchestrator ───────────┴───────────────────────────┐  │
│  │  Receives vision description                         │  │
│  │  Sends screen_share_description to client (progress) │  │
│  │  Injects [LIVE VISION] context into worker model     │  │
│  │  Forwards user's original message + vision context   │  │
│  └──────────────────────────┬───────────────────────────┘  │
│                             │                              │
│  ┌─ Worker Model ───────────┴───────────────────────────┐  │
│  │  Sees: user message + [LIVE VISION] description      │  │
│  │  Analyzes code + visual evidence → proposes fix      │  │
│  │  OR: calls `request_screen_observation` tool         │  │
│  │       → triggers new vision burst (no limit)         │  │
│  └──────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────┘

User clicks [Stop Sharing] → capture stops, ring buffer cleared
```

## The Two-Model Communication — In Detail

This is the make-or-break piece. The vision model must produce output that the worker model can **act on**.

### Vision Model Output Format

The system prompt instructs the vision model to produce structured, machine-readable descriptions:

```
SCREEN STATE:
- Window: Firefox — localhost:3000/dashboard
- Visible error: "Hydration failed because the initial UI does not match 
  what was rendered on the server." (React error overlay, red background)
- Error location: "at Nav (./components/Nav.tsx:42)"
- Temporal: Translation key "nav.home" visible for ~0.3s before "Home" appears

TERMINAL (if visible):
- Last line: "Warning: Text content did not match. Server: "nav.home" Client: "Home""

CHANGES SINCE LAST OBSERVATION:
- Page loaded, error overlay appeared, then disappeared after 0.5s
```

### Worker Model Receives

```
[LIVE VISION — 12:05:03] Observation of window "Firefox — localhost:3000/dashboard":
SCREEN STATE:
- Visible error: "Hydration failed because the initial UI does not match 
  what was rendered on the server." (React error overlay)
- Error location: "at Nav (./components/Nav.tsx:42)"  
- Temporal: Translation key "nav.home" visible for ~0.3s before "Home" appears
TERMINAL: Warning: Text content did not match. Server: "nav.home" Client: "Home"
---
User request: Debug this hydration issue
```

The worker model now has **exact error text**, **file location**, and **temporal behavior** — enough to reason about the bug and propose a fix.

### Worker Model Can Request More

The worker model gets a tool: `request_screen_observation`

```json
{
  "name": "request_screen_observation",
  "description": "Ask the vision model to observe the shared screen again and report what it sees. Use when you need updated visual information to verify a fix or investigate further.",
  "parameters": {
    "focus": "string — what to look for (e.g., 'Check if the hydration error still appears after page reload')"
  }
}
```

When the worker model calls this tool:
1. zcoderd sends a progress message to the client: "🔍 Re-observing screen..."
2. zcoderd re-opens the Gemini Live session (or resumes frame sending)
3. Also sends a `clientContent` prompt to Gemini with the `focus` text
4. Gemini observes, reports
5. zcoderd closes the vision session again
6. The new description is injected as a tool result back to the worker model

This creates a **feedback loop**: worker model proposes fix → user applies it → worker model requests re-observation → vision model confirms fix worked (or reports new issue).

## Corrected Gemini Live API Details

### Model

```
gemini-2.5-flash-native-audio-preview-12-2025
```

This is the **only** current model supporting Live API (BidiGenerateContent) with video input. It's a preview model. When Google releases a stable Live API model, we switch to it.

Alternatives if this model is deprecated before a stable replacement:
- Fall back to standard `generateContent` API with image input (any Gemini model, GPT-4o, Claude)
- The architecture supports this — the orchestrator just needs a different VLM backend

### Session Limits

| Limit | Value | Impact |
|-------|-------|--------|
| Audio+video session | 2 minutes | **Non-issue** — our sessions are seconds long |
| Context window | 32k tokens | At 66 tok/frame × 15 FPS = ~5 seconds fills 5k tokens. Plenty of room |
| Rate limits | Preview-tier (restrictive) | May need to throttle burst frequency |

### Setup Message (corrected)

```json
{
  "setup": {
    "model": "models/gemini-2.5-flash-native-audio-preview-12-2025",
    "generationConfig": {
      "responseModalities": ["TEXT"],
      "mediaResolution": "MEDIA_RESOLUTION_LOW"
    },
    "systemInstruction": {
      "parts": [{ "text": "<vision system prompt — see below>" }]
    }
  }
}
```

No need for `contextWindowCompression` or `sessionResumption` — sessions are too short to need either.

## Vision Model System Prompt (revised)

```
You are a screen observer for a software developer using Zaguán Blade, an AI-powered code editor.

Your output is consumed by a coding AI model, not a human. Be structured and precise.

OUTPUT FORMAT:
SCREEN STATE:
- Window: <title and URL if browser>
- Visible errors: <quote error messages exactly, include file paths and line numbers>
- UI state: <what's displayed — forms, dialogs, loading states, etc.>
- Temporal: <things that appeared/disappeared briefly — flash of content, loading spinners>

TERMINAL (if visible):
- <quote relevant terminal output — errors, warnings, build output>

CHANGES SINCE LAST OBSERVATION:
- <what changed between frames>

RULES:
- Quote error messages and stack traces VERBATIM — the coding model needs exact text
- Include file paths and line numbers whenever visible
- Note temporal events (things visible for <1 second) — these are often the bugs
- Ignore: window chrome, taskbar, desktop, static decorative elements
- If nothing meaningful changed, say "No significant changes observed"
```

## Blade WS Protocol (unchanged from original, with corrections)

### Client → Server

| Message | When | Key fields |
|---------|------|-----------|
| `screen_share_start` | User clicks Share | `session_id`, `mode`, `fps`, `resolution` |
| `screen_share_frame` | Each captured frame | `session_id`, `seq`, `data` (base64 JPEG), `width`, `height` |
| `screen_share_stop` | User clicks Stop | `session_id` |

### Server → Client

| Message | When | Key fields |
|---------|------|-----------|
| `screen_share_status` | Session state changes | `session_id`, `active`, `state`, `error` |
| `screen_share_description` | Vision model reports | `session_id`, `text`, `prompted` |

### New: `state` field in `screen_share_status`

To show progress in the chat, the status message now includes a `state` enum:

```json
{
  "type": "screen_share_status",
  "payload": {
    "session_id": "...",
    "active": true,
    "state": "observing",
    "error": null
  }
}
```

States: `connecting` → `observing` → `processing` → `idle` → `re_observing` → `done`

The client maps these to progress messages in the chat panel.

## API Key & Monetization

**Phase 1 (now): BYOK**
- User provides their own Google AI API key in ZaguanBlade Settings
- Key is sent in `screen_share_start` payload to zcoderd
- zcoderd uses it for the Gemini Live session — does NOT persist it
- Zero cost to Zaguán AI

**Phase 2 (later): zaguancorex billing**
- Per-observation billing (each vision burst = one billable unit)
- Or per-minute of active screen sharing
- BYOK remains as power-user option
- Details TBD once zaguancorex billing is figured out

## Settings UI

Follows the existing `LocalAiSettings` pattern: a bordered card with a `Toggle` + conditionally visible inputs.

### New Settings Section: "Vision"

Add a new sidebar entry in `SettingsModal` between "Local AI" and "Storage":
- **Icon**: `Video` (from lucide-react)
- **Label**: "Vision"
- **Section id**: `'vision'`

### Settings State Changes

Add to `SettingsState`:
```ts
vision: {
    enabled: boolean;
    googleAiApiKey: string;
};
```

Add to `defaultSettings`:
```ts
vision: {
    enabled: false,
    googleAiApiKey: '',
},
```

Add to `ApiConfig` (types/settings.ts) for persistence:
```ts
vision_enabled: boolean;
vision_google_ai_api_key: string;
```

Update `backendGlobalToFrontend` / `frontendGlobalToBackend` mapping functions.

### Vision Settings Component

Layout (mirrors Ollama section in LocalAiSettings):

```
┌─────────────────────────────────────────────────┐
│  Vision / Video Debugger              [Toggle]  │
│  Share your screen with an AI vision model      │
│  to debug visual issues.                        │
│                                                 │
│  Google AI API Key                              │
│  ┌─────────────────────────────────────┐        │
│  │ ••••••••••••••••••••           👁   │        │
│  └─────────────────────────────────────┘        │
│  Get a key at ai.google.dev                     │
│                                                 │
│  [Test Key]                                     │
└─────────────────────────────────────────────────┘
```

- Toggle enables/disables the entire vision feature
- When disabled: API key input is hidden, Share Window button is hidden in the feature menu
- When enabled: API key input is visible with show/hide toggle (same pattern as Account API key)
- "Test Key" button: calls a Tauri command that makes a lightweight Gemini API call to validate the key
- Link to `https://ai.google.dev` for users to get a key

### Gating the Feature

The `useScreenShare` hook (or wherever the Share Window button lives) reads `vision.enabled` from settings. If `false`, the Share Window option is not rendered in the feature dropdown menu. This ensures the feature is completely invisible until the user opts in.

## ZaguanBlade Implementation Scope (this repo)

No changes from the original plan's Phase 1. The capture pipeline, Tauri commands, Blade WS extensions, and frontend UI are the same. The key difference is in how zcoderd uses the frames — which is zcoderd's concern, not ours.

What ZaguanBlade needs:
1. `screen_share.rs` — scap-based capture engine with ring buffer
2. `commands/screen_share.rs` — Tauri commands
3. `blade_ws_client.rs` — new `BladeWsEvent` variants + message types
4. `useScreenShare` hook — frontend state, reads `vision.enabled` from settings
5. `FeatureMenu.tsx` — Share Window option (hidden when vision disabled)
6. `SettingsModal.tsx` — new "Vision" section with toggle + API key input
7. `types/settings.ts` — `vision_enabled` + `vision_google_ai_api_key` in `ApiConfig`
8. Rust backend — new fields in global settings struct + persistence
9. Chat panel — progress messages from `screen_share_status` states
10. Tauri command — `test_vision_api_key` to validate the Google AI key

## Resolved Design Decisions

1. **Reactive only** — The vision model activates ONLY when the user sends a message. Sharing a window just starts the local capture buffer; no frames go to zcoderd/Gemini until the user actually asks something. This keeps costs predictable and avoids noise.

2. **Frame ring buffer** — ZaguanBlade maintains a rolling buffer of the last N seconds of captured frames (e.g., last 5-10 seconds at 15 FPS = 75-150 frames). When the user sends a message, the buffered frames are shipped as a burst to zcoderd. This lets the vision model see what happened *before* the user asked — critical for catching transient glitches that have already disappeared by the time the user types their question.

3. **No re-observation limit** — The worker model can call `request_screen_observation` as many times as needed. The session stays active as long as the user keeps the video share open. The user controls the cost by choosing when to stop sharing. This is consistent with the on-demand philosophy — the user is in control.
