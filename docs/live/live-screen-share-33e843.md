# Live Screen Share with Vision Model

Enable on-demand live screen sharing where ZaguanBlade captures a real-time frame stream and sends it to zcoderd, which routes it to a vision model (Gemini Live API) and injects the resulting descriptions into the worker model's context — all server-side.

## Architecture

```
User clicks "Share Screen" / "Share Window"
        │
        ▼
┌──────────────────────────────────────────────┐
│  ZaguanBlade (Tauri/Rust)                    │
│                                              │
│  xcap captures frame → resize to 720p/360p   │
│  → encode JPEG q75 → base64                  │
│  → send via Blade WS as "screen_frame"       │
│                                              │
│  Configurable FPS: 15 (default) or 30        │
│  User clicks Stop to end session             │
└──────────────┬───────────────────────────────┘
               │ Blade WebSocket (existing)
               ▼
┌──────────────────────────────────────────────┐
│  zcoderd (server)                            │
│                                              │
│  Receives screen_frame messages              │
│  → Manages Gemini Live API WebSocket         │
│  → Sends frames as realtimeInput.video       │
│  → Receives TEXT descriptions from Gemini    │
│  → Injects descriptions into worker model    │
│    context (internal, no round-trip needed)   │
│                                              │
│  Gemini Live API handles context compression │
│  via slidingWindow — continuous streaming OK  │
│                                              │
│  New Blade WS message types:                 │
│  Client→Server:                              │
│  - screen_share_start { mode, window_id?,    │
│    fps, resolution }                         │
│  - screen_share_frame { data, mime_type,     │
│    width, height, seq }                      │
│  - screen_share_stop                         │
│  Server→Client:                              │
│  - screen_share_status { active, error? }    │
│  - screen_share_description { text }         │
│    (optional: show what vision model sees)   │
└──────────────────────────────────────────────┘
```

## Bandwidth & Token Analysis

**Bandwidth (Blade → zcoderd):**

| FPS | 720p JPEG q75 | 360p JPEG q75 |
|-----|---------------|---------------|
| 15  | ~0.5-1 MB/s   | ~150-300 KB/s |
| 30  | ~1-2 MB/s     | ~300-600 KB/s |

All well within LAN/localhost capacity.

**Gemini Live API token consumption:**

| FPS | Low res (66 tok/frame) | Default (258 tok/frame) |
|-----|------------------------|------------------------|
| 15  | 990 tok/s              | 3,870 tok/s            |
| 30  | 1,980 tok/s            | 7,740 tok/s            |

The Live API uses `ContextWindowCompressionConfig` with `slidingWindow` to automatically compress older context when approaching the 32k token limit. This means continuous streaming works — the model just loses detail about older frames, which is fine for our use case (we care about what's on screen *now*).

**Recommendation:** Default to **15 FPS at low media resolution** (66 tokens/frame = 990 tok/s). This catches sub-second visual glitches while keeping token burn manageable. User can bump to 30 FPS for fast-action scenarios.

## Why NOT xcap for Continuous Capture

xcap is designed for single-shot screenshots. Under the hood:

- **X11 path**: Uses `xcb::GetImage` — copies the entire framebuffer over the X11 socket per frame. At 1080p 32bpp = ~8MB per capture. At 15 FPS = 120MB/s of X11 traffic + per-pixel RGBA conversion loop.
- **Wayland path**: Goes through D-Bus → `org.gnome.Shell.Screenshot` → writes PNG to temp file → reads it back → decodes. 15 PNG encode/decode cycles per second hitting disk. Has a global `DBUS_LOCK` mutex serializing everything.

**Using xcap at 15 FPS would destroy the CPU and I/O.**

## The Solution: `scap` crate (forked)

[`scap`](https://crates.io/crates/scap) (v0.1.0-beta.1) is a cross-platform continuous screen capture library. Fork at `ZaguanLabs/scap`, locally at `streaming/scap`.

**xcap stays for single-shot screenshots. scap (forked) for continuous streaming.** No more discussion on swapping.

### Cross-Platform Status

| Platform | scap engine | Status | Notes |
|----------|------------|--------|-------|
| **macOS** | ScreenCaptureKit | ✅ Works | Full feature support: target selection, resolution, crop, FPS, BGRA output |
| **Windows** | Windows.Graphics.Capture | ✅ Works | Full feature support: target selection, crop, BGRA output, audio capture |
| **Linux (Wayland)** | PipeWire + xdg-desktop-portal ScreenCast | ✅ Works | Portal picker for target selection, FPS control, but no resolution/crop |
| **Linux (X11)** | PipeWire + xdg-desktop-portal ScreenCast | ❌ Broken | Portal ScreenCast requires a compositor that implements it (GNOME/KDE/wlroots). Pure X11 WMs like Openbox do NOT have this interface |

**Critical finding:** Dev environment is Mageia 10 + Openbox on X11. PipeWire is running but `xdg-desktop-portal-gtk` does NOT implement ScreenCast. The D-Bus call returns "No such interface". scap's Linux engine will fail at `ScreenCastPortal::create_stream()`.

### X11 Backend — Required Fork Patch

We need to add an X11 capture backend to our scap fork. The system has MIT-SHM, XComposite, and XDamage extensions available.

**Approach: XShm-based continuous capture**

| Method | How it works | CPU at 15 FPS | Pros | Cons |
|--------|-------------|---------------|------|------|
| `XShmGetImage` | Shared memory segment, compositor copies framebuffer directly | ~2-4ms/frame (no socket copy) | 10-50x faster than `XGetImage`, no X11 socket overhead | Still a full-frame copy per capture |
| + XDamage (optional) | Only re-capture when screen content changes | Near-zero when idle | Skips frames when nothing changed | Adds complexity |

Compared to xcap's `XGetImage` (which copies 8MB over the X11 socket per frame), `XShmGetImage` uses shared memory — the compositor writes directly to a buffer the client can read. At 720p BGRA = ~3.7MB, this is a memcpy, not a socket transfer.

**Implementation plan for X11 backend in scap fork:**

1. New file: `src/capturer/engine/linux/x11.rs`
2. Detect X11 vs Wayland at runtime (`$XDG_SESSION_TYPE` or `$WAYLAND_DISPLAY`)
3. X11 path: XShm + timer-based capture at configured FPS
4. Wayland path: existing PipeWire/portal code (unchanged)
5. Both paths deliver frames through the same `mpsc::Sender<Frame>` channel

**Dependencies to add (Linux, X11 path):** `x11rb` crate with `shm` feature (or raw `xcb` bindings)

This is a significant upstream-worthy contribution — scap currently has zero X11 support.

## scap v0.1.0-beta.1 Gap Analysis

Evaluated the full source for all three platform engines.

### macOS (ScreenCaptureKit) — Ready
- Target selection via `SCShareableContent` — windows + displays with full metadata
- Resolution control via `SCStreamConfiguration` width/height
- Crop via `source_rect`
- FPS via `minimum_frame_interval`
- BGRA output supported
- Proper `SystemTime` timestamps

### Windows (Graphics.Capture) — Ready
- Target selection via `windows-capture` crate — windows + monitors
- Crop via `buffer_crop()`
- BGRA output natively
- Audio capture supported
- Proper `SystemTime` timestamps

### Linux — Needs Work

**Wayland path (existing):** Works on GNOME/KDE/Sway where portal ScreenCast is available.

**X11 path (missing):** Does not exist. Must be added to our fork.

**Common Linux issues (both paths):**

| Issue | Workaround |
|-------|-----------|
| `CAPTURER_STATE` is global static — only one session at a time | Fine for us — one share at a time |
| No BGRA in PipeWire format negotiation | Use BGRx, swap B↔R before JPEG encode |
| No `output_resolution` on Linux | Resize in Rust after capture (~1-2ms at 720p) |
| `get_all_targets()` returns empty on Linux | X11: we can enumerate via xcb; Wayland: portal picker |
| `display_time` is raw PTS, not `SystemTime` | We don't need precise timestamps |

**Decision: Fork scap.** Fork lives at `ZaguanLabs/scap` on GitHub and locally at `streaming/scap`. Any improvements we make that benefit the scap project get upstreamed as PRs to `CapSoftware/scap`.

### Fork Branch Strategy

```
main              ← tracks upstream CapSoftware/scap (sync periodically)
zaguan            ← our working branch (main + all feature branches merged)
feat/x11-backend  ← upstream PR: X11 capture via XShm (biggest contribution)
feat/bgra-linux   ← upstream PR: add BGRA to PipeWire format negotiation
feat/per-instance ← upstream PR: per-instance capturer state (replace global AtomicU8)
```

- Feature branches fork off `main`, each = one upstream PR
- `zaguan` merges `main` + all feature branches — Cargo.toml points here
- When upstream accepts a PR, delete branch, rebase `zaguan` on updated `main`

### Cargo.toml Reference

- **During development**: `scap = { path = "../streaming/scap" }` (fast iteration)
- **For releases/CI**: `scap = { git = "ssh://git@github.com/ZaguanLabs/scap.git", branch = "zaguan" }`
- Use `.cargo/config.toml` `[patch]` to switch between them without editing Cargo.toml

### Upstream PR Plan

| PR | Description | Effort | Priority |
|----|-------------|--------|----------|
| **#1 X11 backend** | XShm-based capture with runtime X11/Wayland detection | Large (~200-300 lines) | **Critical — blocks dev on our system** |
| #2 BGRA format on Linux | Add `VideoFormat::BGRA` to PipeWire negotiation | ~3 lines | Small |
| #3 Per-instance state | Replace global `static CAPTURER_STATE` with per-instance | ~20 lines | Small |

## Scope Split: ZaguanBlade vs zcoderd

### ZaguanBlade (this repo) — what we build here

1. **New dependency: `scap`** (`Cargo.toml`)
   - Add `scap` v0.1.0-beta.1 via git dependency pointing to our fork
   - System requirement: `libpipewire-0.3-dev` on Linux
   - xcap stays for single-shot screenshots (existing feature, unchanged)
   - Upstream-worthy patches get PRed back to `CapSoftware/scap`

2. **UI: FeatureMenu additions** (`FeatureMenu.tsx`)
   - New "Vision" section with two items: "Share Screen" and "Share Window"
   - When active, item changes to "Stop Sharing" with a pulsing red dot indicator
   - "Share Window" uses scap's target selection (displays + windows)
   - FPS selector (15/30) in the menu or a settings panel

3. **Frame capture engine** (new Rust module: `src-tauri/src/screen_share.rs`)
   - Creates a `scap::Capturer` with configured FPS (15/30)
   - Platform frame delivery:
     - **Linux X11**: XShm capture on timer thread → BGRA frames
     - **Linux Wayland**: PipeWire via portal → BGRx frames
     - **macOS**: ScreenCaptureKit → BGRA frames
     - **Windows**: Graphics.Capture → BGRA frames
   - Pipeline per frame: pixel format normalize → resize to 720p/360p via `image` crate (~1-2ms) → JPEG encode q75 (~2-4ms) → base64 → send via Blade WS
   - Total per-frame CPU: ~3-6ms — well within 66ms budget at 15 FPS
   - Frame sequencing (monotonic counter) so zcoderd can detect drops
   - Runs on a dedicated `std::thread` (capture loops are blocking)
   - Controlled by start/stop via `tokio::sync::watch` channel
   - Back-pressure: if WS send is slow, drop frames rather than queue them

4. **Tauri commands** (`src-tauri/src/commands/screen_share.rs`)
   - `start_screen_share(mode: "screen" | "window", target_id: Option<u32>, fps: u8, resolution: String)` — begins capture
   - `stop_screen_share()` — stops capture
   - `get_screen_share_status()` — returns `{ active, fps, resolution, frames_sent, elapsed_secs }`
   - `list_screen_share_targets()` — returns available displays + windows via scap

5. **Blade WS protocol extensions** (`blade_ws_client.rs`)
   - `send_screen_share_start(mode, target_id, fps, resolution)` — tells zcoderd to open Gemini session
   - `send_screen_frame(data, mime_type, width, height, seq)` — ships a frame
   - `send_screen_share_stop()` — tells zcoderd to close Gemini session
   - New incoming event: `BladeWsEvent::ScreenShareStatus { active, error }`
   - New incoming event: `BladeWsEvent::ScreenShareDescription { text }` (optional)

6. **Frontend state management** (`CommandCenter.tsx` + new hook)
   - `useScreenShare` hook: manages active/inactive state, FPS, resolution, frame count
   - Status indicator in CommandCenter header bar when sharing is active (pulsing dot + "Sharing")
   - Optional: show latest vision model description in a collapsible panel or tooltip

### zcoderd (separate repo) — what needs to happen there

- New Gemini Live API WebSocket client (server-to-server approach)
- Session config: `responseModalities: ["TEXT"]`, `mediaResolution: "low"`, `contextWindowCompression: { slidingWindow: {} }`
- Frame routing: receive `screen_frame` → forward to Gemini as `realtimeInput.video`
- Description routing: Gemini TEXT response → inject into worker model context
- Session resumption for long-running shares (auto-resume when GoAway received)
- Config: Google AI API key management (user settings)
- New WS message types to match the client protocol above

## Implementation Order

**Phase 0: scap fork patches (in `streaming/scap`)**
0a. **X11 backend** — `src/capturer/engine/linux/x11.rs` + runtime detection
0b. **BGRA format** — add to PipeWire negotiation list
0c. **Per-instance state** — replace global `CAPTURER_STATE`

**Phase 1: ZaguanBlade integration**
1. **Rust: `Cargo.toml`** — add `scap = { path = "../streaming/scap" }`
2. **Rust: `screen_share.rs`** — scap-based capture engine with JPEG encode + WS send
3. **Rust: `commands/screen_share.rs`** — Tauri command wrappers
4. **Rust: `blade_ws_client.rs`** — new WS message types for screen sharing
5. **Rust: wire into `main.rs`** — register new commands
6. **Frontend: `useScreenShare` hook** — state management + Tauri invoke calls
7. **Frontend: `FeatureMenu.tsx`** — UI for start/stop screen sharing
8. **Frontend: `CommandCenter.tsx`** — status indicator + wire up the hook

## Key Technical Decisions

- **Capture library**: `scap` (forked) for continuous streaming; `xcap` stays for single-shot screenshots only
- **Linux X11 support**: XShm-based backend added to scap fork (critical — dev environment is Openbox/X11)
- **Frame rate**: 15 FPS default, 30 FPS option. Catches sub-second visual glitches (e.g. hydration flashes)
- **Resolution**: 720p default, 360p option. Resize in Rust after capture on Linux; native on macOS/Windows
- **Encoding**: JPEG at quality 75 (~30-60KB per frame at 720p, ~2-4ms CPU per frame)
- **CPU budget**: At 15 FPS, capture (~2-4ms XShm) + resize (~1-2ms) + JPEG encode (~2-4ms) = ~5-10ms/frame. Well within 66ms budget
- **Back-pressure**: If WS send can't keep up, drop frames (never queue). Monotonic sequence counter lets zcoderd detect gaps
- **No audio**: Vision-only; audio adds complexity with no benefit for a code editor
- **On-demand only**: User explicitly starts/stops from the Feature Menu
- **zcoderd owns all AI logic**: Blade captures and ships frames; never talks to Gemini directly
- **Dedicated thread**: Capture loop runs on `std::thread`, bridges to tokio via channels
- **Cross-platform**: macOS (ScreenCaptureKit), Windows (Graphics.Capture), Linux Wayland (PipeWire), Linux X11 (XShm)

## Gemini Live API Reference (for zcoderd implementation)

- **WebSocket**: `wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent`
- **Video input**: `{ "realtimeInput": { "video": { "mimeType": "image/jpeg", "data": "<base64>" } } }`
- **Response modality**: TEXT (not AUDIO)
- **Context management**: `ContextWindowCompressionConfig` with `slidingWindow` — auto-compresses older frames
- **Session resumption**: `SessionResumptionConfig` with handle for seamless reconnection
- **GoAway**: Server sends `GoAway` with `timeLeft` before disconnecting — client should resume
- **Model**: `gemini-2.0-flash` or `gemini-2.5-flash` (multimodal, supports Live API)
- **Token rates**: 66 tokens/frame (low res), 258 tokens/frame (default res)
