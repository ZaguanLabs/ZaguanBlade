# ideai Blade Protocol Refactoring - Implementation Status

**Date**: December 30, 2025  
**Version**: 0.0.1-alpha  
**Status**: Phase 1 Complete, Phase 2 In Progress

---

## Completed Today

### ✅ zcoderd - Web Tools Backend (100% Complete)

**Aggregator Service**:
- Async job API with progress tracking
- Job manager with cleanup
- Routes: `POST /v1/jobs`, `GET /v1/jobs/{jobID}`

**Web Tools** (3 tools implemented):
1. **fetch_url** (@web) - Single URL with 7-day cache
2. **search** (@search) - Quick SearXNG search
3. **research** (@research) - Full aggregator pipeline with grading

**Infrastructure**:
- Web tools executor with progress polling
- Server-side tool execution
- Progress SSE events
- Configuration system
- All builds successful ✅

**Documentation**:
- Single consolidated Blade Protocol spec
- Web tools design document
- Quick reference guide
- Example configuration

### ✅ ideai - Progress Event Support (100% Complete)

**Modified Files**:
- `protocol.rs` - Added `Progress` variant to `ChatEvent`
- `chat/api.rs` - Added progress event parsing
- `chat_manager.rs` - Added progress handling in drain_events
- Build successful ✅

### ✅ Phase 1: Blade Protocol Client (100% Complete)

**Created Files**:
- `blade_client.rs` (~300 lines)
  - `BladeClient` struct
  - `BladeEvent` enum (Session, Text, ToolCall, Progress, Done, Error)
  - `WorkspaceInfo`, `ToolResult` types
  - SSE stream parsing
  - `send_message()` and `send_tool_result()` methods

**Status**: Builds successfully with 3 minor warnings (harmless)

---

## Next Steps - Phase 2: Refactor chat_manager.rs

### Current State
`chat_manager.rs` currently:
- Calls Anthropic API directly (`launch_request`)
- Calls Responses API directly (`launch_responses_request`)
- Routes between APIs based on model type
- Builds system prompts
- Handles XML parsing for tool calls
- ~677 lines of complex code

### Target State
`chat_manager.rs` should:
- Call Blade Protocol only (`launch_blade_request`)
- No API routing (zcoderd handles this)
- No prompt building (zcoderd handles this)
- No XML parsing (zcoderd handles this)
- ~300 lines of simple code (55% reduction)

### Required Changes

#### 1. Update `start_stream()` signature
**Before**:
```rust
pub fn start_stream(
    &mut self,
    _prompt: String,
    conversation: &mut ConversationHistory,
    api_config: &ApiConfig,
    models: &[ModelInfo],
    selected_model: usize,
    workspace: Option<&PathBuf>,
    http: reqwest::Client,
) -> Result<(), String>
```

**After**:
```rust
pub fn start_stream(
    &mut self,
    conversation: &mut ConversationHistory,
    blade_url: &str,
    model_id: String,
    workspace: Option<&PathBuf>,
    http: reqwest::Client,
) -> Result<(), String>
```

**Changes**:
- Remove `_prompt` (not used)
- Remove `api_config` (no API key needed)
- Remove `models` array and `selected_model` index
- Add `blade_url` string
- Add `model_id` string directly

#### 2. Simplify `start_stream()` implementation
**Remove**:
- Model routing logic (`uses_responses_api()`, `uses_anthropic_endpoint()`)
- System prompt building (`prompts::common::get_system_prompt()`)
- API-specific calls (`launch_request()`, `launch_responses_request()`)

**Add**:
```rust
pub fn start_stream(
    &mut self,
    conversation: &mut ConversationHistory,
    blade_url: &str,
    model_id: String,
    workspace: Option<&PathBuf>,
    http: reqwest::Client,
) -> Result<(), String> {
    self.in_think_block = false;
    self.xml_buffer.clear();

    // Build workspace info
    let workspace_info = blade_client::WorkspaceInfo {
        root: workspace
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        active_file: None,
        cursor_position: None,
        open_files: Vec::new(),
    };

    // Get last user message
    let user_message = conversation
        .iter()
        .rev()
        .find(|m| m.role == ChatRole::User)
        .map(|m| m.content.clone())
        .unwrap_or_default();

    // Create Blade client
    let blade_client = blade_client::BladeClient::new(blade_url.to_string(), http);

    // Send message and get event stream
    let rx = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(blade_client.send_message(
            self.session_id.clone(),
            model_id,
            user_message,
            workspace_info,
            HashMap::new(),
        ))?;

    // Convert BladeEvent channel to ChatEvent channel
    let (tx, chat_rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            let chat_event = match event {
                blade_client::BladeEvent::Session { session_id, .. } => {
                    // Store session_id somehow
                    continue;
                }
                blade_client::BladeEvent::Text(text) => ChatEvent::Chunk(text),
                blade_client::BladeEvent::ToolCall { id, name, arguments } => {
                    // Convert to ToolCall
                    ChatEvent::ToolCalls(vec![/* ... */])
                }
                blade_client::BladeEvent::Progress { message, stage, percent } => {
                    ChatEvent::Progress { message, stage, percent }
                }
                blade_client::BladeEvent::Done { .. } => ChatEvent::Done,
                blade_client::BladeEvent::Error { message, .. } => ChatEvent::Error(message),
                _ => continue,
            };
            let _ = tx.send(chat_event);
        }
    });

    // Push placeholder for assistant response
    conversation.push(ChatMessage {
        tool_calls: None,
        role: ChatRole::Assistant,
        content: String::new(),
        reasoning: None,
        tool_call_id: None,
    });

    self.rx = Some(chat_rx);
    self.streaming = true;
    Ok(())
}
```

#### 3. Update `continue_tool_batch()`
Similar simplification - use Blade Protocol `send_tool_result()` instead of rebuilding API requests.

#### 4. Remove dead methods
- `launch_request()` - Anthropic API
- `launch_responses_request()` - Responses API
- All prompt building logic
- All XML parsing logic (keep minimal for backward compat if needed)

---

## Phase 3: Remove Dead Code

### Files to Delete (~80KB)
1. `chat/api.rs` (20KB)
2. `chat/responses_api.rs` (18KB)
3. `responses_protocol.rs` (5KB)
4. `chat/message_builder.rs` (6KB)
5. `prompts/common.rs`
6. `prompts/qwen.rs`
7. `prompts/gemini.rs`
8. `prompts/gpt52.rs`
9. `models/model_support.rs`
10. `xml_parser.rs` (17KB) - Optional, may keep for legacy

### Update `lib.rs`
Remove module declarations for deleted files.

---

## Phase 4: Update Configuration

### Current `config.rs`
```rust
pub struct ApiConfig {
    pub api_key: String,
    pub api_url: String,
}
```

### New `config.rs`
```rust
pub struct ApiConfig {
    pub blade_url: String,  // Default: "http://localhost:8880"
}
```

### UI Changes
- Remove API key input field
- Add Blade URL input field
- Simplify model selection (just show model IDs)

---

## Phase 5: Testing

### Test Cases
- [ ] Basic conversation works
- [ ] Tool calls execute correctly
- [ ] Tool results sent back
- [ ] Progress events display
- [ ] Session persistence
- [ ] Error handling
- [ ] File operations
- [ ] Command execution
- [ ] Web tools (server-side)

### Prerequisites
1. zcoderd running at `http://localhost:8880`
2. Aggregator running at `http://localhost:8080`
3. SearXNG running at `http://127.0.0.1:8888`

---

## Estimated Time Remaining

- **Phase 2**: Refactor chat_manager.rs - 2 hours
- **Phase 3**: Remove dead code - 1 hour
- **Phase 4**: Update UI config - 1 hour
- **Phase 5**: Testing - 2 hours

**Total**: ~6 hours

---

## Current Build Status

✅ **zcoderd**: Builds successfully  
✅ **aggregator**: Builds successfully  
✅ **ideai**: Builds successfully (3 harmless warnings)

---

## Files Modified Today

### zcoderd (11 new, 8 modified)
**New**:
- `internal/aggregator/client.go`
- `internal/webtools/fetcher.go`
- `internal/webtools/cache.go`
- `internal/webtools/searxng.go`
- `internal/blade/webtools_executor.go`
- `internal/blade/webtools_handler.go`
- `aggregator/internal/domain/progress.go`
- `aggregator/internal/jobs/manager.go`
- `docs/blade-protocol.md`
- `docs/web-tools-design.md`
- `docs/web-tools-quick-reference.md`

**Modified**:
- `internal/config/config.go`
- `internal/blade/handler.go`
- `internal/blade/chat.go`
- `internal/blade/streaming.go`
- `internal/api/handler.go`
- `aggregator/internal/pipeline/orchestrator.go`
- `aggregator/internal/api/handler.go`
- `aggregator/cmd/aggregator/main.go`

### ideai (1 new, 3 modified)
**New**:
- `src-tauri/src/blade_client.rs`

**Modified**:
- `src-tauri/src/protocol.rs`
- `src-tauri/src/chat/api.rs`
- `src-tauri/src/chat_manager.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/Cargo.toml` (version bump)

---

**Ready to continue with Phase 2 when you return!**
