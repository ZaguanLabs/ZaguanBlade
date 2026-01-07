# Visual Debugging System Design
**"The Eyes of the Blade" - Multimodal AI-Powered Visual Debugging**

Date: 2026-01-01  
Status: Design Phase  
Priority: **GAME CHANGER**

---

## The Vision

Transform Zaguán Blade from a code editor into a **"Digital Pair Programmer with Eyes"** that can:
- See the developer's screen in real-time
- Watch applications run and identify visual bugs
- Understand UI/UX issues that can't be described in text
- Debug across the entire development environment (browser, terminal, database GUI, etc.)
- Provide context-aware fixes based on what it **sees**, not just what it reads

---

## Why This is Revolutionary

### Current State of AI Coding Assistants
- **Text-only context**: AI only knows what you tell it
- **"It doesn't work" problem**: Developers struggle to describe visual bugs
- **Limited environment awareness**: AI can't see browser console, network tab, etc.
- **Async debugging**: Developer describes bug → AI guesses → developer tests → repeat

### With Visual Debugging
- **Full visual context**: AI sees exactly what you see
- **"Show, don't tell"**: Just run the app, trigger the bug, AI watches
- **Complete environment**: Browser, terminal, database, everything
- **Real-time debugging**: AI sees the bug happen and suggests fix immediately

### The Killer Use Cases

1. **CSS/Layout Bugs**
   - "That button isn't clickable" → AI sees the transparent overlay
   - "The layout breaks on mobile" → AI watches the responsive behavior
   - "There's a weird flicker" → AI catches the race condition visually

2. **Runtime Errors**
   - AI watches browser console as you trigger the bug
   - AI sees the network tab showing failed requests
   - AI observes the React DevTools component tree

3. **UX Issues**
   - "The animation feels janky" → AI sees the frame drops
   - "The loading state is confusing" → AI watches the user flow
   - "The form validation is unclear" → AI sees the user experience

4. **Cross-Tool Debugging**
   - AI sees your database GUI showing wrong data
   - AI watches your terminal showing build errors
   - AI observes your browser and editor side-by-side

5. **The Recursive Fix**
   - AI proposes a UI change
   - AI watches the change render in real-time
   - AI sees it's broken and immediately suggests a fix
   - **The AI debugs itself**

---

## Technical Architecture

### High-Level Flow

```
┌─────────────────────────────────────────────────────────────┐
│                     Developer's Screen                       │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   Browser    │  │   Terminal   │  │   Database   │      │
│  │   (App UI)   │  │   (Logs)     │  │   (GUI)      │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
                            ↓
                    Screen Capture Service
                    (Rust/Tauri - zblade)
                            ↓
                    Frame Processing Pipeline
                    (Go - zcoderd)
                            ↓
                    Multimodal AI API
                    (Claude 3.5 Sonnet Vision)
                            ↓
                    Visual Analysis + Code Context
                            ↓
                    AI Response with Fixes
```

### Component Breakdown

#### 1. Screen Capture Service (zblade - Rust/Tauri)
**Responsibility:** Capture screen/window frames and send to backend

**Technology Options:**
- `scrap` crate - Cross-platform screen capture
- `captrs` crate - Fast screenshot library
- `screenshots` crate - Multi-monitor support
- Native Tauri APIs - Window-specific capture

**Modes:**
- **Full screen capture** - Everything the developer sees
- **Window capture** - Specific window (browser, terminal, etc.)
- **Region capture** - User-selected area
- **Multi-monitor** - Capture from specific display

**Capture Strategies:**
- **On-demand**: User clicks "Show AI" button
- **Continuous**: Stream frames at 1-5 FPS during debugging session
- **Event-triggered**: Capture when error occurs, test fails, etc.
- **Smart**: Only capture when screen changes (diff detection)

#### 2. Frame Processing Pipeline (zcoderd - Go)
**Responsibility:** Receive frames, optimize, and route to AI

**Processing Steps:**
1. **Receive** - Accept base64 or binary frames from zblade
2. **Compress** - Reduce size while maintaining readability (JPEG, WebP)
3. **Annotate** - Add metadata (timestamp, window title, cursor position)
4. **Buffer** - Store recent frames for context (last 30 seconds)
5. **Route** - Send to appropriate multimodal AI endpoint

**Optimization:**
- **Frame diffing**: Only send frames that changed significantly
- **Adaptive quality**: Lower quality for continuous streaming, high for snapshots
- **Region of interest**: Crop to relevant area (e.g., just the browser)
- **Caching**: Avoid re-sending identical frames

#### 3. Multimodal AI Integration
**Responsibility:** Send visual + code context to AI, receive analysis

**Supported Models:**
- **Claude 3.5 Sonnet** (Anthropic) - Best vision + coding
- **GPT-4o** (OpenAI) - Strong multimodal
- **Gemini 1.5 Pro** (Google) - Long context + vision
- **Qwen2-VL** (Alibaba) - Open-source option

**Request Format:**
```json
{
  "model": "claude-3-5-sonnet-20241022",
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "image",
          "source": {
            "type": "base64",
            "media_type": "image/jpeg",
            "data": "<base64_encoded_screenshot>"
          }
        },
        {
          "type": "text",
          "text": "I'm seeing a bug in this UI. The 'Submit' button doesn't respond to clicks. Here's the relevant code:\n\n```tsx\n<button onClick={handleSubmit}>Submit</button>\n```\n\nWhat's wrong?"
        }
      ]
    }
  ]
}
```

**Context Enrichment:**
- Combine screenshot with:
  - Current file content
  - Recent git changes
  - Console logs (if captured)
  - Network requests (if captured)
  - Stack traces

#### 4. Visual Artifact Storage
**Responsibility:** Store visual history for replay and analysis

**Storage Strategy:**
- **MariaDB**: Metadata (timestamp, session_id, window_title, tags)
- **Object Storage**: Actual frames (S3-compatible, local filesystem)
- **Redis**: Recent frames cache (last 5 minutes)

**Schema:**
```sql
CREATE TABLE visual_artifacts (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    session_id VARCHAR(64) NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    frame_type ENUM('screenshot', 'video_frame', 'screen_recording'),
    window_title VARCHAR(255),
    storage_path VARCHAR(512),
    file_size INT,
    width INT,
    height INT,
    metadata JSON,
    ai_analysis TEXT,
    INDEX idx_session_timestamp (session_id, timestamp)
);
```

**Retention Policy:**
- Keep last 24 hours in Redis (fast access)
- Keep last 7 days in full quality
- Compress/downsample older frames
- Delete after 30 days (configurable)

---

## Implementation Details

### Phase 1: Basic Screenshot Capture

#### zblade (Rust/Tauri)

**File: `src-tauri/src/screen_capture.rs` (NEW)**

```rust
use screenshots::Screen;
use base64::{Engine as _, engine::general_purpose};
use image::ImageFormat;
use std::io::Cursor;

pub struct ScreenCapture {
    screens: Vec<Screen>,
}

impl ScreenCapture {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let screens = Screen::all()?;
        Ok(Self { screens })
    }
    
    /// Capture primary screen as base64 JPEG
    pub fn capture_primary_screen(&self) -> Result<String, Box<dyn std::error::Error>> {
        let screen = &self.screens[0];
        let image = screen.capture()?;
        
        // Convert to JPEG for compression
        let mut buffer = Cursor::new(Vec::new());
        image.save_with_format(&mut buffer, ImageFormat::Jpeg)?;
        
        // Encode to base64
        let base64_data = general_purpose::STANDARD.encode(buffer.into_inner());
        
        Ok(base64_data)
    }
    
    /// Capture specific window by title
    pub fn capture_window(&self, window_title: &str) -> Result<String, Box<dyn std::error::Error>> {
        // Platform-specific window capture
        // On macOS: Use CGWindowListCreateImage
        // On Linux: Use X11/Wayland APIs
        // On Windows: Use Windows.Graphics.Capture
        
        todo!("Window-specific capture")
    }
    
    /// Capture region of screen
    pub fn capture_region(&self, x: i32, y: i32, width: u32, height: u32) 
        -> Result<String, Box<dyn std::error::Error>> {
        let screen = &self.screens[0];
        let full_image = screen.capture()?;
        
        // Crop to region
        let cropped = image::imageops::crop_imm(
            &full_image, 
            x as u32, 
            y as u32, 
            width, 
            height
        ).to_image();
        
        // Convert to base64
        let mut buffer = Cursor::new(Vec::new());
        cropped.save_with_format(&mut buffer, ImageFormat::Jpeg)?;
        let base64_data = general_purpose::STANDARD.encode(buffer.into_inner());
        
        Ok(base64_data)
    }
}

// Tauri command
#[tauri::command]
pub async fn capture_screen() -> Result<String, String> {
    let capture = ScreenCapture::new()
        .map_err(|e| format!("Failed to initialize screen capture: {}", e))?;
    
    let screenshot = capture.capture_primary_screen()
        .map_err(|e| format!("Failed to capture screen: {}", e))?;
    
    Ok(screenshot)
}

#[tauri::command]
pub async fn capture_window_by_title(title: String) -> Result<String, String> {
    let capture = ScreenCapture::new()
        .map_err(|e| format!("Failed to initialize screen capture: {}", e))?;
    
    let screenshot = capture.capture_window(&title)
        .map_err(|e| format!("Failed to capture window: {}", e))?;
    
    Ok(screenshot)
}
```

**Dependencies to add:**
```toml
# Cargo.toml
[dependencies]
screenshots = "0.8"
image = "0.24"
base64 = "0.21"
```

**Register commands:**
```rust
// src-tauri/src/main.rs
mod screen_capture;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // ... existing commands
            screen_capture::capture_screen,
            screen_capture::capture_window_by_title,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

#### Frontend (React/TypeScript)

**File: `src/components/VisualDebugger.tsx` (NEW)**

```typescript
import React, { useState } from 'react';
import { invoke } from '@tauri-apps/api';
import { Camera, Eye, Monitor } from 'lucide-react';

export const VisualDebugger: React.FC = () => {
    const [isCapturing, setIsCapturing] = useState(false);
    const [lastScreenshot, setLastScreenshot] = useState<string | null>(null);

    const captureScreen = async () => {
        setIsCapturing(true);
        try {
            const base64Image = await invoke<string>('capture_screen');
            setLastScreenshot(`data:image/jpeg;base64,${base64Image}`);
            
            // Send to AI for analysis
            await sendToAI(base64Image);
        } catch (e) {
            console.error('Failed to capture screen:', e);
        } finally {
            setIsCapturing(false);
        }
    };

    const sendToAI = async (imageBase64: string) => {
        // Send to zcoderd for AI analysis
        // This will be implemented in Phase 2
        console.log('Sending screenshot to AI...');
    };

    return (
        <div className="visual-debugger">
            <button
                onClick={captureScreen}
                disabled={isCapturing}
                className="flex items-center gap-2 px-3 py-2 bg-purple-600 hover:bg-purple-500 text-white rounded"
            >
                <Camera className="w-4 h-4" />
                {isCapturing ? 'Capturing...' : 'Show AI My Screen'}
            </button>
            
            {lastScreenshot && (
                <div className="mt-4">
                    <p className="text-xs text-zinc-400 mb-2">Last Capture:</p>
                    <img 
                        src={lastScreenshot} 
                        alt="Screenshot" 
                        className="max-w-full border border-zinc-700 rounded"
                    />
                </div>
            )}
        </div>
    );
};
```

### Phase 2: AI Integration

#### zcoderd (Go)

**File: `internal/vision/capture.go` (NEW)**

```go
package vision

import (
    "encoding/base64"
    "time"
)

type ScreenCapture struct {
    ID          string    `json:"id"`
    SessionID   string    `json:"session_id"`
    Timestamp   time.Time `json:"timestamp"`
    ImageBase64 string    `json:"image_base64"`
    WindowTitle string    `json:"window_title,omitempty"`
    Width       int       `json:"width"`
    Height      int       `json:"height"`
}

type CaptureService struct {
    storage *CaptureStorage
}

func NewCaptureService(storage *CaptureStorage) *CaptureService {
    return &CaptureService{storage: storage}
}

func (s *CaptureService) ProcessCapture(capture *ScreenCapture) error {
    // 1. Store in database
    if err := s.storage.Save(capture); err != nil {
        return err
    }
    
    // 2. Add to recent cache (Redis)
    if err := s.storage.CacheRecent(capture); err != nil {
        return err
    }
    
    return nil
}
```

**File: `internal/vision/analysis.go` (NEW)**

```go
package vision

import (
    "context"
    "fmt"
    
    "github.com/ZaguanAI/zcoderd/internal/blade"
)

type VisualAnalyzer struct {
    client *blade.AnthropicClient
}

func NewVisualAnalyzer(apiKey string) *VisualAnalyzer {
    return &VisualAnalyzer{
        client: blade.NewAnthropicClient(apiKey),
    }
}

func (a *VisualAnalyzer) AnalyzeScreenshot(
    ctx context.Context,
    imageBase64 string,
    userQuery string,
    codeContext string,
) (string, error) {
    
    // Build multimodal message
    messages := []blade.BladeMessage{
        {
            Role: "user",
            Content: []blade.ContentBlock{
                {
                    Type: "image",
                    Source: &blade.ImageSource{
                        Type:      "base64",
                        MediaType: "image/jpeg",
                        Data:      imageBase64,
                    },
                },
                {
                    Type: "text",
                    Text: fmt.Sprintf(
                        "I'm debugging an issue. Here's what I'm seeing on my screen.\n\n"+
                        "User's question: %s\n\n"+
                        "Relevant code:\n```\n%s\n```\n\n"+
                        "Please analyze the screenshot and the code to identify the issue.",
                        userQuery,
                        codeContext,
                    ),
                },
            },
        },
    }
    
    // Call Claude with vision
    response, err := a.client.SendMessage(ctx, messages, "claude-3-5-sonnet-20241022")
    if err != nil {
        return "", err
    }
    
    return response.Content[0].Text, nil
}
```

**File: `internal/blade/anthropic.go` (UPDATE)**

```go
// Add image support to existing AnthropicRequest

type ImageSource struct {
    Type      string `json:"type"`       // "base64"
    MediaType string `json:"media_type"` // "image/jpeg", "image/png"
    Data      string `json:"data"`       // base64 encoded image
}

type ContentBlock struct {
    Type   string       `json:"type"` // "text" or "image"
    Text   string       `json:"text,omitempty"`
    Source *ImageSource `json:"source,omitempty"`
}

type AnthropicMessage struct {
    Role    string         `json:"role"`
    Content []ContentBlock `json:"content"` // Changed from string to []ContentBlock
}
```

### Phase 3: Continuous Streaming

**File: `src-tauri/src/screen_capture.rs` (UPDATE)**

```rust
use tokio::time::{interval, Duration};
use tokio::sync::mpsc;

pub struct StreamingCapture {
    capture: ScreenCapture,
    fps: u32,
}

impl StreamingCapture {
    pub fn new(fps: u32) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            capture: ScreenCapture::new()?,
            fps,
        })
    }
    
    /// Start streaming frames
    pub async fn start_stream(
        &self,
        tx: mpsc::Sender<String>
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut interval = interval(Duration::from_millis(1000 / self.fps as u64));
        
        loop {
            interval.tick().await;
            
            match self.capture.capture_primary_screen() {
                Ok(frame) => {
                    if tx.send(frame).await.is_err() {
                        break; // Channel closed
                    }
                }
                Err(e) => {
                    eprintln!("Frame capture error: {}", e);
                }
            }
        }
        
        Ok(())
    }
}

#[tauri::command]
pub async fn start_screen_stream(
    app_handle: tauri::AppHandle,
    fps: u32
) -> Result<(), String> {
    let (tx, mut rx) = mpsc::channel(10);
    
    // Start capture in background
    tokio::spawn(async move {
        let stream = StreamingCapture::new(fps).unwrap();
        stream.start_stream(tx).await.ok();
    });
    
    // Forward frames to frontend
    tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            app_handle.emit_all("screen-frame", frame).ok();
        }
    });
    
    Ok(())
}
```

### Phase 4: Smart Capture Triggers

**Automatic capture on:**
- Console errors
- Test failures
- Build errors
- Network request failures
- React error boundaries
- User clicks "Help" button

**File: `src/hooks/useVisualDebugger.ts` (NEW)**

```typescript
import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api';
import { listen } from '@tauri-apps/api/event';

export const useVisualDebugger = () => {
    useEffect(() => {
        // Listen for console errors
        const originalError = console.error;
        console.error = (...args) => {
            originalError(...args);
            captureOnError(args.join(' '));
        };
        
        // Listen for unhandled promise rejections
        window.addEventListener('unhandledrejection', (event) => {
            captureOnError(event.reason);
        });
        
        return () => {
            console.error = originalError;
        };
    }, []);
    
    const captureOnError = async (errorMessage: string) => {
        try {
            const screenshot = await invoke<string>('capture_screen');
            // Send to AI with error context
            await sendErrorToAI(screenshot, errorMessage);
        } catch (e) {
            console.error('Failed to capture on error:', e);
        }
    };
};
```

---

## Advanced Features

### 1. Browser DevTools Integration

Capture not just the UI, but also:
- Console logs
- Network tab
- React DevTools component tree
- Performance metrics

**Implementation:**
- Use Chrome DevTools Protocol (CDP)
- Connect to browser's debugging port
- Extract structured data alongside screenshot

### 2. Multi-Window Awareness

Track which window is active:
- Browser (app UI)
- Terminal (logs/errors)
- Database GUI
- Editor (code)

**Implementation:**
- Use OS APIs to detect active window
- Capture relevant window automatically
- Include window title in AI context

### 3. Visual Diff

Compare before/after screenshots:
- Before applying AI's fix
- After applying AI's fix
- Highlight visual differences

**Implementation:**
- Use image diffing algorithm
- Highlight changed regions
- Show side-by-side comparison

### 4. Session Replay

Record entire debugging session:
- All screenshots in sequence
- All code changes
- All AI suggestions
- Timeline view

**Implementation:**
- Store frames with timestamps
- Build video from frames
- Annotate with events (code edits, AI responses)

### 5. Collaborative Debugging

Share visual debugging session:
- Stream to remote developer
- Stream to AI in real-time
- Multiple developers see same screen

**Implementation:**
- WebRTC for low-latency streaming
- Shared session ID
- Real-time collaboration protocol

---

## Privacy & Security

### Concerns

1. **Sensitive data in screenshots** (passwords, API keys, personal info)
2. **Bandwidth usage** (continuous streaming)
3. **Storage costs** (storing many frames)
4. **Privacy regulations** (GDPR, CCPA)

### Solutions

1. **Opt-in by default**
   - User must explicitly enable visual debugging
   - Clear indicator when capturing
   - Easy to pause/stop

2. **Redaction**
   - Detect and blur sensitive fields (password inputs, etc.)
   - Allow user to mark regions as "never capture"
   - OCR to detect API keys/secrets and redact

3. **Local-first**
   - Process frames locally when possible
   - Only send to cloud when necessary
   - Option for fully local AI (Ollama with vision models)

4. **Encryption**
   - Encrypt frames in transit (TLS)
   - Encrypt frames at rest
   - End-to-end encryption for collaborative sessions

5. **Retention controls**
   - User-configurable retention period
   - One-click delete all captures
   - Auto-delete after session ends

---

## Cost Analysis

### API Costs (Claude 3.5 Sonnet Vision)

**Pricing:**
- Input: $3 per million tokens
- Images: ~1,600 tokens per image (approximate)
- Output: $15 per million tokens

**Scenarios:**

1. **On-demand screenshot** (1 image + 500 tokens text)
   - Input: ~2,100 tokens = $0.0063
   - Output: ~500 tokens = $0.0075
   - **Total: ~$0.014 per query**

2. **Continuous streaming** (1 FPS for 5 minutes = 300 frames)
   - Input: 300 × 1,600 = 480,000 tokens = $1.44
   - Output: ~5,000 tokens = $0.075
   - **Total: ~$1.52 per 5-minute session**

3. **Smart capture** (10 captures per hour)
   - Input: 10 × 2,100 = 21,000 tokens = $0.063
   - Output: 10 × 500 = 5,000 tokens = $0.075
   - **Total: ~$0.14 per hour**

**Optimization strategies:**
- Use lower resolution for continuous streaming
- Only send frames when significant change detected
- Batch multiple frames into single request
- Use cheaper models for initial triage (GPT-4o mini)

### Storage Costs

**Assumptions:**
- JPEG screenshot: ~200 KB
- 10 captures per day
- 30-day retention

**Storage:**
- Per user: 200 KB × 10 × 30 = 60 MB/month
- 1,000 users: 60 GB/month
- S3 Standard: ~$1.38/month for 1,000 users

**Negligible compared to API costs**

---

## Competitive Analysis

### Existing Solutions

1. **GitHub Copilot** - No vision
2. **Cursor** - No vision
3. **Windsurf** - No vision (yet?)
4. **Replit AI** - No vision
5. **Google Gemini** - Has vision, but not IDE-integrated

### Zaguán Blade's Advantage

**First mover in IDE-integrated visual debugging**
- Only editor that can "see" what you see
- Only editor that debugs across entire environment
- Only editor with visual artifact replay

**This is a moat.** Once developers experience debugging by showing instead of telling, they won't go back.

---

## Go-to-Market Strategy

### Positioning

**"The AI That Sees What You See"**

### Target Audience

1. **Frontend developers** - Visual bugs are their daily pain
2. **Full-stack developers** - Need to debug across browser/server
3. **DevOps engineers** - Need to debug deployment issues visually
4. **QA engineers** - Visual regression testing

### Pricing Tiers

1. **Free tier** - 10 visual queries per day
2. **Pro tier** ($20/month) - Unlimited queries, 7-day retention
3. **Team tier** ($50/user/month) - Collaborative sessions, 30-day retention
4. **Enterprise** (Custom) - On-premise, unlimited retention

### Marketing Hooks

- "Show, don't tell your AI"
- "The AI that debugs what it sees"
- "Stop describing bugs, start showing them"
- "Your AI pair programmer just got eyes"

---

## Implementation Roadmap

### Phase 0: Quick Screenshot Capture (1 week) ⭐ **PRIORITY**

**Goal:** Streamline the screenshot workflow - eliminate file system browsing friction

**Current Pain Point:**
1. Take screenshot with system tool
2. Click tiny + button in chat
3. Browse file system to find screenshot
4. Select file
5. Describe problem in detail
6. Send

**New Workflow:**
1. Click "📷 Capture" button (or press `Ctrl+Shift+S`)
2. Select region/window to capture
3. Screenshot automatically attached to chat input
4. Type description (optional - AI can analyze image)
5. Send

**Reduction: 6 steps → 3 steps, no file system navigation**

#### Implementation Details

**Option A: System Tool Integration (Recommended)**

Leverage existing OS screenshot tools:
- **Linux**: `gnome-screenshot`, `spectacle`, `flameshot`
- **macOS**: `screencapture` command
- **Windows**: `Snipping Tool` API

**Advantages:**
- Users already familiar with these tools
- Rich features (region select, annotations, delays)
- No need to reinvent the wheel
- Smaller binary size

**Implementation:**
```rust
// src-tauri/src/screenshot.rs
use std::process::Command;
use std::path::PathBuf;

pub struct QuickScreenshot;

impl QuickScreenshot {
    /// Capture screenshot using system tool, return temp file path
    pub fn capture_with_system_tool() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let temp_path = std::env::temp_dir().join(format!("zblade_screenshot_{}.png", 
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs()
        ));
        
        #[cfg(target_os = "linux")]
        {
            // Try flameshot first (best UX), fallback to gnome-screenshot
            if Command::new("flameshot").arg("--version").output().is_ok() {
                Command::new("flameshot")
                    .args(["gui", "-p", temp_path.to_str().unwrap()])
                    .status()?;
            } else if Command::new("gnome-screenshot").arg("--version").output().is_ok() {
                Command::new("gnome-screenshot")
                    .args(["-a", "-f", temp_path.to_str().unwrap()])
                    .status()?;
            } else {
                return Err("No screenshot tool found. Install flameshot or gnome-screenshot".into());
            }
        }
        
        #[cfg(target_os = "macos")]
        {
            Command::new("screencapture")
                .args(["-i", temp_path.to_str().unwrap()])
                .status()?;
        }
        
        #[cfg(target_os = "windows")]
        {
            // Use Windows.Graphics.Capture API via PowerShell
            Command::new("powershell")
                .args([
                    "-Command",
                    &format!("Add-Type -AssemblyName System.Windows.Forms; \
                             [System.Windows.Forms.SendKeys]::SendWait('%{{PRTSC}}'); \
                             Start-Sleep -Milliseconds 500; \
                             Get-Clipboard -Format Image | \
                             Set-Content -Path '{}' -Encoding Byte", 
                             temp_path.display())
                ])
                .status()?;
        }
        
        // Verify file was created
        if temp_path.exists() {
            Ok(temp_path)
        } else {
            Err("Screenshot was cancelled or failed".into())
        }
    }
    
    /// Read screenshot file and convert to base64
    pub fn read_as_base64(path: &PathBuf) -> Result<String, Box<dyn std::error::Error>> {
        let bytes = std::fs::read(path)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
    }
    
    /// Cleanup temp file
    pub fn cleanup(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::remove_file(path)?;
        Ok(())
    }
}

#[tauri::command]
pub async fn quick_screenshot() -> Result<String, String> {
    // Capture using system tool
    let temp_path = QuickScreenshot::capture_with_system_tool()
        .map_err(|e| e.to_string())?;
    
    // Read as base64
    let base64_data = QuickScreenshot::read_as_base64(&temp_path)
        .map_err(|e| e.to_string())?;
    
    // Cleanup temp file
    QuickScreenshot::cleanup(&temp_path).ok();
    
    Ok(base64_data)
}
```

**Option B: Built-in Capture (Fallback)**

Use `screenshots` crate for cross-platform capture:

```rust
use screenshots::Screen;

#[tauri::command]
pub async fn quick_screenshot_builtin() -> Result<String, String> {
    let screens = Screen::all()
        .map_err(|e| format!("Failed to get screens: {}", e))?;
    
    let screen = &screens[0];
    let image = screen.capture()
        .map_err(|e| format!("Failed to capture: {}", e))?;
    
    // Convert to base64
    let mut buffer = std::io::Cursor::new(Vec::new());
    image.save_with_format(&mut buffer, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode: {}", e))?;
    
    Ok(base64::engine::general_purpose::STANDARD.encode(buffer.into_inner()))
}
```

**Hybrid Approach (Best):**
- Try system tool first (better UX)
- Fallback to built-in if system tool not available
- User can configure preference in settings

#### Frontend Integration

**File: `src/components/ChatInput.tsx` (UPDATE)**

```typescript
import { Camera } from 'lucide-react';
import { invoke } from '@tauri-apps/api';

interface ChatInputProps {
    onSend: (message: string, attachments?: Attachment[]) => void;
    // ... other props
}

interface Attachment {
    type: 'image';
    data: string; // base64
    preview: string; // data URL for preview
}

export const ChatInput: React.FC<ChatInputProps> = ({ onSend, ... }) => {
    const [attachments, setAttachments] = useState<Attachment[]>([]);
    const [isCapturing, setIsCapturing] = useState(false);
    
    const handleQuickScreenshot = async () => {
        setIsCapturing(true);
        try {
            const base64Image = await invoke<string>('quick_screenshot');
            
            // Add to attachments
            setAttachments(prev => [...prev, {
                type: 'image',
                data: base64Image,
                preview: `data:image/png;base64,${base64Image}`
            }]);
            
            // Optional: Show toast notification
            console.log('Screenshot captured!');
        } catch (e) {
            console.error('Screenshot failed:', e);
            // Show error toast
        } finally {
            setIsCapturing(false);
        }
    };
    
    const removeAttachment = (index: number) => {
        setAttachments(prev => prev.filter((_, i) => i !== index));
    };
    
    const handleSend = () => {
        if (message.trim() || attachments.length > 0) {
            onSend(message, attachments);
            setMessage('');
            setAttachments([]);
        }
    };
    
    return (
        <div className="chat-input-container">
            {/* Attachment previews */}
            {attachments.length > 0 && (
                <div className="flex gap-2 p-2 bg-zinc-900/50 border-t border-zinc-800">
                    {attachments.map((att, idx) => (
                        <div key={idx} className="relative group">
                            <img 
                                src={att.preview} 
                                alt="Screenshot" 
                                className="w-20 h-20 object-cover rounded border border-zinc-700"
                            />
                            <button
                                onClick={() => removeAttachment(idx)}
                                className="absolute -top-1 -right-1 w-5 h-5 bg-red-600 rounded-full text-white text-xs opacity-0 group-hover:opacity-100 transition-opacity"
                            >
                                ×
                            </button>
                        </div>
                    ))}
                </div>
            )}
            
            {/* Input area */}
            <div className="flex items-end gap-2 p-3">
                <textarea
                    value={message}
                    onChange={(e) => setMessage(e.target.value)}
                    placeholder="Ask anything or attach a screenshot..."
                    className="flex-1 ..."
                />
                
                {/* Screenshot button */}
                <button
                    onClick={handleQuickScreenshot}
                    disabled={isCapturing}
                    title="Capture Screenshot (Ctrl+Shift+S)"
                    className="p-2 rounded hover:bg-zinc-700 transition-colors"
                >
                    <Camera className={`w-5 h-5 ${isCapturing ? 'animate-pulse' : ''}`} />
                </button>
                
                {/* Send button */}
                <button onClick={handleSend} className="...">
                    Send
                </button>
            </div>
        </div>
    );
};
```

#### Keyboard Shortcut

**File: `src/hooks/useKeyboardShortcuts.ts` (UPDATE)**

```typescript
import { useEffect } from 'react';

export const useKeyboardShortcuts = (onScreenshot: () => void) => {
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // Ctrl+Shift+S (or Cmd+Shift+S on Mac)
            if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === 'S') {
                e.preventDefault();
                onScreenshot();
            }
        };
        
        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [onScreenshot]);
};
```

#### Backend Integration (zcoderd)

**File: `internal/blade/streaming.go` (UPDATE)**

Update message handling to support image attachments:

```go
type MessageContent struct {
    Type   string       `json:"type"` // "text" or "image"
    Text   string       `json:"text,omitempty"`
    Source *ImageSource `json:"source,omitempty"`
}

type ImageSource struct {
    Type      string `json:"type"`       // "base64"
    MediaType string `json:"media_type"` // "image/png"
    Data      string `json:"data"`       // base64 string
}

// When building request to Claude
func buildMultimodalMessage(text string, images []string) BladeMessage {
    content := []MessageContent{
        {
            Type: "text",
            Text: text,
        },
    }
    
    // Add images
    for _, imgBase64 := range images {
        content = append(content, MessageContent{
            Type: "image",
            Source: &ImageSource{
                Type:      "base64",
                MediaType: "image/png",
                Data:      imgBase64,
            },
        })
    }
    
    return BladeMessage{
        Role:    "user",
        Content: content,
    }
}
```

#### User Experience Flow

**Scenario 1: Quick Bug Report**
1. User sees a bug in their app
2. Presses `Ctrl+Shift+S`
3. System screenshot tool opens (e.g., flameshot)
4. User selects the buggy region
5. Screenshot appears as thumbnail in chat input
6. User types: "The button is misaligned"
7. Sends → AI sees screenshot + description
8. AI responds with fix

**Scenario 2: Multiple Screenshots**
1. User captures first screenshot (error message)
2. User captures second screenshot (console logs)
3. User captures third screenshot (network tab)
4. All three appear as thumbnails
5. User types: "These three are related"
6. Sends → AI analyzes all three together

**Scenario 3: Screenshot-Only**
1. User captures screenshot
2. User sends without typing anything
3. AI analyzes image and asks: "I see an error dialog. What would you like me to help with?"

#### Settings & Configuration

**File: `src/components/Settings.tsx` (UPDATE)**

```typescript
interface ScreenshotSettings {
    tool: 'system' | 'builtin' | 'auto';
    systemToolPath?: string; // Custom path to screenshot tool
    autoCleanup: boolean;     // Delete temp files after send
    quality: 'high' | 'medium' | 'low'; // Compression level
}

// Settings UI
<div className="setting-group">
    <h3>Screenshot Capture</h3>
    
    <label>
        <span>Capture Method</span>
        <select value={settings.screenshot.tool}>
            <option value="auto">Auto (system tool if available)</option>
            <option value="system">System Tool Only</option>
            <option value="builtin">Built-in Capture</option>
        </select>
    </label>
    
    <label>
        <span>Image Quality</span>
        <select value={settings.screenshot.quality}>
            <option value="high">High (larger file, better quality)</option>
            <option value="medium">Medium (balanced)</option>
            <option value="low">Low (smaller file, faster upload)</option>
        </select>
    </label>
    
    <label>
        <input 
            type="checkbox" 
            checked={settings.screenshot.autoCleanup}
        />
        <span>Auto-cleanup temp files</span>
    </label>
</div>
```

#### Error Handling

**Common Issues & Solutions:**

1. **No screenshot tool found (Linux)**
   - Show notification: "Install flameshot or gnome-screenshot for better experience"
   - Fallback to built-in capture
   - Provide install instructions in settings

2. **Permission denied**
   - macOS: Request screen recording permission
   - Linux: Check Wayland vs X11 compatibility
   - Show clear error message with fix instructions

3. **User cancelled screenshot**
   - Don't show error
   - Just log to console
   - No attachment added

4. **File too large**
   - Compress image before sending
   - Show warning if >5MB
   - Offer to reduce quality

#### Testing Checklist

- [ ] Screenshot capture works on Linux (X11)
- [ ] Screenshot capture works on Linux (Wayland)
- [ ] Screenshot capture works on macOS
- [ ] Screenshot capture works on Windows
- [ ] Keyboard shortcut works (Ctrl+Shift+S)
- [ ] Multiple screenshots can be attached
- [ ] Screenshots can be removed before sending
- [ ] Screenshots are sent to AI correctly
- [ ] AI can analyze screenshots
- [ ] Temp files are cleaned up
- [ ] Error handling works gracefully
- [ ] Settings persist across restarts

#### Success Metrics

- **Time to attach screenshot**: <5 seconds (vs ~30 seconds with file browser)
- **User satisfaction**: "Much easier" rating >90%
- **Screenshot usage**: 3x increase in screenshots sent to AI
- **Bug resolution time**: 20% faster with visual context

---

### Phase 1: Proof of Concept (1-2 weeks)
- [ ] Basic screenshot capture in zblade
- [ ] Send to Claude 3.5 Sonnet vision API
- [ ] Display AI analysis in chat
- [ ] Test with real bugs

### Phase 2: Core Features (2-3 weeks)
- [ ] Window-specific capture
- [ ] Region selection
- [ ] Frame storage in MariaDB
- [ ] Visual artifact viewer
- [ ] Integration with existing chat

### Phase 3: Smart Capture (2 weeks)
- [ ] Auto-capture on errors
- [ ] Browser DevTools integration
- [ ] Multi-window awareness
- [ ] Diff detection (only send changed frames)

### Phase 4: Advanced Features (3-4 weeks)
- [ ] Continuous streaming mode
- [ ] Session replay
- [ ] Visual diff (before/after)
- [ ] Collaborative debugging
- [ ] Privacy controls (redaction)

### Phase 5: Polish & Launch (2 weeks)
- [ ] Performance optimization
- [ ] Cost optimization
- [ ] Security audit
- [ ] Documentation
- [ ] Marketing materials
- [ ] Beta testing

**Total:**
- **Phase 0 (Quick Screenshot)**: 1 week → **Immediate value**
- **Phases 1-5 (Full Visual Debugging)**: 10-13 weeks → **Game changer**

**Recommendation:** Implement Phase 0 first for quick wins, then tackle full system

---

## Technical Risks & Mitigations

### Risk 1: Performance Impact
**Problem:** Screen capture might slow down development  
**Mitigation:** 
- Async capture in background thread
- Configurable FPS (default: 1 FPS)
- Pause during intensive tasks

### Risk 2: API Costs
**Problem:** Continuous streaming could be expensive  
**Mitigation:**
- Smart frame diffing (only send changes)
- User-controlled streaming (manual start/stop)
- Cost caps and warnings

### Risk 3: Privacy Concerns
**Problem:** Users might not trust sending screenshots  
**Mitigation:**
- Clear opt-in flow
- Visual indicator when capturing
- Local-only mode option
- Redaction tools

### Risk 4: Model Limitations
**Problem:** AI might not understand complex visual bugs  
**Mitigation:**
- Combine vision with code context
- Allow user to annotate screenshots
- Fallback to text-only debugging

### Risk 5: Platform Compatibility
**Problem:** Screen capture APIs differ across OS  
**Mitigation:**
- Use cross-platform `screenshots` crate
- Graceful degradation on unsupported platforms
- Platform-specific optimizations

---

## Success Metrics

### Technical Metrics
- Screenshot capture latency: <100ms
- AI response time: <5 seconds
- Frame processing throughput: >10 FPS
- Storage efficiency: <100 MB per user per month

### User Metrics
- Visual queries per user per day
- Bug resolution time (with vs without vision)
- User satisfaction (NPS score)
- Feature adoption rate

### Business Metrics
- Conversion rate (free → paid)
- Monthly recurring revenue
- Customer acquisition cost
- Churn rate

---

## Future Vision

### The "Autonomous Debugger"

Imagine:
1. Developer runs their app
2. AI watches in background
3. AI detects visual bug automatically
4. AI proposes fix
5. AI applies fix
6. AI watches to confirm fix worked
7. **Developer never had to say anything**

This is the endgame: **Fully autonomous visual debugging.**

### The "Recursive Observer"

The AI watches:
- The code it writes
- The UI it generates
- The bugs it creates
- The fixes it applies

**The AI becomes self-correcting.**

### The "Collaborative Swarm"

Multiple AIs watching:
- One AI watches the frontend
- One AI watches the backend
- One AI watches the database
- They collaborate to fix cross-stack bugs

**The AI becomes a team.**

---

## Conclusion

This isn't just a feature. **This is a paradigm shift.**

You're not building a better code editor. You're building the first **"Sense-and-Respond"** development environment.

Every other AI coding assistant is blind. Zaguán Blade will be the first to see.

**This is the billion-dollar idea.**

---

**Next Steps:**
1. Build Phase 1 proof of concept
2. Test with real bugs
3. Validate the approach
4. Iterate based on feedback
5. Scale to production

**The Eyes of the Blade are opening.** 👁️

---

**End of Design Document**
