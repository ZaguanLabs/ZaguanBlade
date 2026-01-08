# ideai Blade Protocol Refactoring Plan

**Date**: December 30, 2025  
**Goal**: Transform ideai from a direct AI provider client to a thin Blade Protocol client

---

## Current Architecture (Problems)

### What ideai Does Now (WRONG)
1. **Direct AI Provider Calls**
   - Calls Anthropic API directly (`chat/api.rs`)
   - Calls Responses API directly (`chat/responses_api.rs`)
   - Manages API keys, headers, authentication
   - Handles different API formats (Anthropic vs Responses)

2. **Prompt Management**
   - Builds system prompts (`prompts/`)
   - Model-specific instructions
   - Tool definitions
   - Message formatting

3. **Model Routing**
   - Decides which API to use based on model
   - `uses_responses_api()`, `uses_anthropic_endpoint()`

4. **Complex SSE Parsing**
   - Anthropic-specific SSE events
   - Responses API-specific SSE events
   - Different formats for each provider

### What's Dead Code Now
- ❌ `chat/api.rs` - Direct Anthropic API calls
- ❌ `chat/responses_api.rs` - Direct Responses API calls
- ❌ `responses_protocol.rs` - Responses API types
- ❌ `prompts/*` - All prompt building (zcoderd does this)
- ❌ `models/model_support.rs` - Model routing logic
- ❌ `chat/message_builder.rs` - API message formatting
- ❌ XML parsing for tool calls (zcoderd handles this)

---

## Target Architecture (Blade Protocol Client)

### What ideai SHOULD Do
1. **Single Blade Protocol Endpoint**
   - Call `POST /v1/blade/chat` only
   - Send simple Blade Protocol messages
   - No API key management (zcoderd handles it)

2. **Thin SSE Client**
   - Parse unified Blade Protocol SSE events:
     - `session` - Session info
     - `text` - Text chunks
     - `tool_call` - Tool execution requests
     - `progress` - Progress updates
     - `done` - Completion
     - `error` - Errors

3. **Tool Executor**
   - Execute tools locally (file ops, commands)
   - Send results back via `POST /v1/blade/chat` (type: tool_result)
   - Web tools execute server-side (no client action needed)

4. **Simple Message Format**
   ```rust
   struct BladeRequest {
       session_id: Option<String>,
       type: String,  // "message" or "tool_result"
       model_id: String,
       content: String,
       workspace: WorkspaceInfo,
       file_hashes: HashMap<String, String>,
       tool_call_id: Option<String>,
       result: Option<ToolResult>,
   }
   ```

---

## Refactoring Steps

### Phase 1: Create Blade Protocol Client (NEW)

**Create `blade_client.rs`**:
```rust
pub struct BladeClient {
    base_url: String,
    http_client: reqwest::Client,
}

impl BladeClient {
    pub async fn send_message(
        &self,
        session_id: Option<String>,
        model_id: String,
        content: String,
        workspace: WorkspaceInfo,
    ) -> Result<mpsc::Receiver<BladeEvent>, String>

    pub async fn send_tool_result(
        &self,
        session_id: String,
        tool_call_id: String,
        result: ToolResult,
    ) -> Result<mpsc::Receiver<BladeEvent>, String>

    fn parse_sse_stream(
        &self,
        response: Response,
        tx: mpsc::Sender<BladeEvent>,
    ) -> Result<(), String>
}

pub enum BladeEvent {
    Session { session_id: String, model: String },
    Text(String),
    ToolCall { id: String, name: String, arguments: Value },
    Progress { message: String, stage: String, percent: i32 },
    Done,
    Error(String),
}
```

### Phase 2: Simplify ChatManager

**Modify `chat_manager.rs`**:
- Remove `launch_request()` (Anthropic)
- Remove `launch_responses_request()` (Responses)
- Replace with `launch_blade_request()`
- Remove model routing logic
- Remove prompt building
- Simplify to just call Blade Protocol

**New `start_stream()`**:
```rust
pub fn start_stream(
    &mut self,
    conversation: &mut ConversationHistory,
    blade_url: &str,
    model_id: String,
    workspace: Option<&PathBuf>,
    http: reqwest::Client,
) -> Result<(), String> {
    let blade_client = BladeClient::new(blade_url, http);
    
    // Get last user message
    let user_message = conversation.last_user_message();
    
    // Build workspace info
    let workspace_info = WorkspaceInfo {
        root: workspace.map(|p| p.to_string_lossy().to_string()),
        // ... other fields
    };
    
    // Send to Blade Protocol
    let rx = blade_client.send_message(
        self.session_id.clone(),
        model_id,
        user_message,
        workspace_info,
    ).await?;
    
    self.rx = Some(rx);
    self.streaming = true;
    Ok(())
}
```

### Phase 3: Update Event Handling

**Modify `drain_events()`**:
```rust
pub fn drain_events(&mut self, conversation: &mut ConversationHistory) -> DrainResult {
    while let Some(event) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
        match event {
            BladeEvent::Session { session_id, .. } => {
                self.session_id = Some(session_id);
            }
            BladeEvent::Text(text) => {
                // Append to assistant message
                if let Some(last) = conversation.last_mut() {
                    last.content.push_str(&text);
                }
            }
            BladeEvent::ToolCall { id, name, arguments } => {
                // Return tool calls for execution
                tool_calls.push(ToolCall { id, name, arguments });
            }
            BladeEvent::Progress { message, .. } => {
                // Append progress to assistant message
                if let Some(last) = conversation.last_mut() {
                    last.content.push_str(&format!("\n{}\n", message));
                }
            }
            BladeEvent::Done => {
                done = true;
            }
            BladeEvent::Error(e) => {
                error_msg = Some(e);
                done = true;
            }
        }
    }
    // Return result
}
```

### Phase 4: Remove Dead Code

**Delete Files**:
- ❌ `chat/api.rs` (20KB)
- ❌ `chat/responses_api.rs` (18KB)
- ❌ `responses_protocol.rs` (5KB)
- ❌ `prompts/` directory (all files)
- ❌ `models/model_support.rs`
- ❌ `chat/message_builder.rs`

**Simplify Files**:
- `protocol.rs` - Keep only `ChatMessage`, `ToolCall`, remove API-specific types
- `chat_manager.rs` - Remove 50% of code (API routing, prompt building)
- `xml_parser.rs` - Can be removed (zcoderd handles this)

### Phase 5: Update Configuration

**Modify `config.rs`**:
```rust
pub struct ApiConfig {
    pub blade_url: String,  // "http://localhost:8880"
    // Remove: api_key, api_url, anthropic_version, etc.
}
```

**Update UI config**:
- Remove API key input
- Add Blade URL input (default: `http://localhost:8880`)
- Simplify model selection (just ID, no API routing)

### Phase 6: Tool Execution

**Keep `tools.rs`** but simplify:
- File operations (read, write, patch, list)
- Command execution
- Remove any AI-specific logic

**Tool result sending**:
```rust
pub async fn send_tool_result(
    blade_client: &BladeClient,
    session_id: String,
    tool_call_id: String,
    result: ToolResult,
) -> Result<(), String> {
    blade_client.send_tool_result(session_id, tool_call_id, result).await
}
```

---

## Code Reduction Estimate

### Before Refactoring
- Total lines: ~15,000
- Files: 25+
- Complexity: High (3 API formats, routing, prompts)

### After Refactoring
- Total lines: ~6,000 (60% reduction)
- Files: 15 (40% reduction)
- Complexity: Low (1 protocol, simple SSE)

### Files to Delete (8)
1. `chat/api.rs` - 20KB
2. `chat/responses_api.rs` - 18KB
3. `responses_protocol.rs` - 5KB
4. `chat/message_builder.rs` - 6KB
5. `prompts/common.rs`
6. `prompts/qwen.rs`
7. `prompts/gemini.rs`
8. `prompts/gpt52.rs`
9. `models/model_support.rs`
10. `xml_parser.rs` - 17KB

**Total dead code**: ~80KB

### Files to Create (1)
1. `blade_client.rs` - ~5KB

### Files to Simplify (3)
1. `chat_manager.rs` - 26KB → 12KB (50% reduction)
2. `protocol.rs` - Keep minimal types
3. `config.rs` - Simplify to just blade_url

---

## Benefits

### For Users
✅ Simpler configuration (just Blade URL)
✅ No API key management in client
✅ Consistent behavior (zcoderd handles all AI logic)
✅ Better error messages (from zcoderd)

### For Developers
✅ 60% less code to maintain
✅ Single protocol to understand
✅ No API version tracking
✅ Easier testing (mock Blade Protocol)
✅ Clear separation of concerns

### For System
✅ zcoderd = AI orchestration, prompts, model routing
✅ ideai = UI, tool execution, user interaction
✅ Clean architecture (thin client, smart server)

---

## Migration Path

### Step 1: Implement Blade Client (2 hours)
- Create `blade_client.rs`
- Implement SSE parsing
- Test with zcoderd

### Step 2: Refactor ChatManager (2 hours)
- Replace API calls with Blade calls
- Update event handling
- Test conversation flow

### Step 3: Remove Dead Code (1 hour)
- Delete unused files
- Clean up imports
- Remove dead functions

### Step 4: Update UI (1 hour)
- Remove API key input
- Add Blade URL config
- Simplify model selection

### Step 5: Testing (2 hours)
- Test all tools
- Test progress events
- Test error handling
- Test session management

**Total Estimate**: 8 hours

---

## Testing Checklist

- [ ] Basic conversation works
- [ ] Tool calls execute correctly
- [ ] Tool results sent back properly
- [ ] Progress events display during research
- [ ] Session persistence works
- [ ] Error handling works
- [ ] File operations work
- [ ] Command execution works
- [ ] Web tools work (server-side)
- [ ] Multiple conversations work

---

## Risks & Mitigation

### Risk 1: Breaking Changes
**Mitigation**: Keep old code in `legacy/` folder temporarily

### Risk 2: Missing Features
**Mitigation**: Document any features lost, add to zcoderd if needed

### Risk 3: Performance
**Mitigation**: Blade Protocol is designed for streaming, should be faster

---

**End of Refactoring Plan**
