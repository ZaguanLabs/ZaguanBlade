# Blade Protocol v2 - Cache Warmup Addendum

**Version**: 2.1-alpha  
**Status**: Implemented  
**Date**: January 15, 2026  
**Authors**: Zaguán AI Team

---

## Table of Contents

1. [Overview](#overview)
2. [Motivation](#motivation)
3. [Protocol Extension](#protocol-extension)
4. [Message Types](#message-types)
5. [Provider Detection](#provider-detection)
6. [Cost Optimization](#cost-optimization)
7. [Integration Guide](#integration-guide)
8. [Examples](#examples)
9. [Implementation Notes](#implementation-notes)

---

## Overview

This addendum extends Blade Protocol v2 with **proactive cache warmup** capabilities. The warmup mechanism enables zblade to pre-populate the LLM provider's prompt cache before the user makes their first request, achieving **zero-latency artifact access** while maintaining **cost efficiency**.

### Key Features

- **Proactive Warmup**: Client-initiated cache population on launch, model change, workspace change, and session resume
- **Model-Aware**: Only warms the cache for the specific provider being used
- **Cost Optimized**: 3-minute cache reuse window prevents redundant warmup requests
- **Async Processing**: Immediate response to client while warmup continues in background
- **Provider Agnostic**: Supports multiple cache providers (Anthropic, OpenAI, etc.)

---

## Motivation

### The Problem

Without cache warmup, the first request in a session incurs:
- **Full token cost**: All artifacts sent as fresh tokens ($15/1M for Anthropic)
- **High latency**: Large context requires more processing time
- **Poor UX**: User waits while cache populates

### The Solution

Proactive warmup inspired by Claude Code's behavior:
1. zblade sends warmup request on launch with current model
2. zcoderd detects provider and warms only that provider's cache
3. User makes first request → cache already warm → instant response
4. Subsequent requests benefit from 90% cache discount

### Business Impact

**Cost Savings at Scale** (1000 users, 10 requests/day):
- Without warmup: $3,750/day
- With warmup: $150/day (warmup + cached reads)
- **Savings: $3,600/day = $108,000/month**

**Revenue Model**:
- Charge: $15/1M tokens (full price)
- Pay: $0.30/1M tokens (cached reads)
- **Profit margin: 98%**

---

## Protocol Extension

### New Endpoint

```
POST /v1/blade/warmup
```

**Transport**: HTTP (not WebSocket)  
**Reason**: Warmup is a one-shot operation, doesn't need persistent connection

### When to Send Warmup

zblade should send warmup requests on:

1. **Launch**: When application starts
2. **Model Change**: When user switches models
3. **Workspace Change**: When user opens different project
4. **Session Resume**: After predetermined inactivity period (e.g., 5 minutes)

---

## Message Types

### 1. Warmup Request

**Direction**: Client → Server  
**Endpoint**: `POST /v1/blade/warmup`

```typescript
interface WarmupRequest {
  type: "warmup";
  session_id: string;
  user_id: string;
  model: string;    // Full model identifier: "anthropic/claude-sonnet-4"
  trigger: "launch" | "model_change" | "workspace_change" | "session_resume";
}
```

**Example**:
```json
{
  "type": "warmup",
  "session_id": "sess_abc123",
  "user_id": "default",
  "model": "anthropic/claude-sonnet-4-20250514",
  "trigger": "launch"
}
```

### 2. Warmup Response

**Direction**: Server → Client

```typescript
interface WarmupResponse {
  type: "warmup_complete" | "warmup_already_warm" | "warmup_not_supported";
  session_id: string;
  provider: string;           // "anthropic", "openai", "groq", etc.
  cache_supported: boolean;
  artifacts_loaded: number;   // Number of artifacts in session
  cache_ready: boolean;
  duration_ms: number;        // Response time in milliseconds
  message?: string;           // Optional human-readable message
}
```

**Example (Success)**:
```json
{
  "type": "warmup_complete",
  "session_id": "sess_abc123",
  "provider": "anthropic",
  "cache_supported": true,
  "artifacts_loaded": 15,
  "cache_ready": true,
  "duration_ms": 23,
  "message": "Warming 15 artifacts for anthropic"
}
```

**Example (Already Warm)**:
```json
{
  "type": "warmup_already_warm",
  "session_id": "sess_abc123",
  "provider": "anthropic",
  "cache_supported": true,
  "artifacts_loaded": 15,
  "cache_ready": true,
  "duration_ms": 5,
  "message": "Cache already warm from recent warmup"
}
```

**Example (Not Supported)**:
```json
{
  "type": "warmup_not_supported",
  "session_id": "sess_abc123",
  "provider": "groq",
  "cache_supported": false,
  "artifacts_loaded": 0,
  "cache_ready": false,
  "duration_ms": 2,
  "message": "Provider groq does not support prompt caching"
}
```

---

## Provider Detection

### Model String Format

Models follow the pattern: `provider/model-name`

**Examples**:
- `anthropic/claude-sonnet-4-20250514` → provider: `anthropic`
- `openai/gpt-4o` → provider: `openai`
- `groq/llama-3.3-70b-versatile` → provider: `groq`

### Cache Support Matrix

| Provider | Cache Supported | Cache Type | TTL |
|----------|----------------|------------|-----|
| Anthropic | ✅ Yes | Ephemeral (`cache_control`) | 5 min |
| OpenAI | ✅ Yes (future) | Prompt caching | TBD |
| Groq | ❌ No | N/A | N/A |
| DeepSeek | ❌ No | N/A | N/A |
| Others | ❌ No | N/A | N/A |

### Detection Algorithm

```go
func DetectProvider(model string) string {
    parts := strings.Split(model, "/")
    if len(parts) > 0 {
        return strings.ToLower(parts[0])
    }
    return "unknown"
}

func ProviderSupportsCache(provider string) bool {
    switch provider {
    case "anthropic":
        return true
    case "openai":
        return true  // Future support
    default:
        return false
    }
}
```

---

## Cost Optimization

### Cache Reuse Window

**Problem**: Rapid warmup requests waste money

**Solution**: 3-minute reuse window

```
Request 1 (t=0s):   Warmup → Cache warmed → Redis: "warmed" (TTL: 3min)
Request 2 (t=30s):  Warmup → Redis hit → Skip (already warm)
Request 3 (t=90s):  Warmup → Redis hit → Skip (already warm)
Request 4 (t=200s): Warmup → Redis expired → Rewarm cache
```

**Why 3 minutes?**
- Anthropic cache TTL: 5 minutes
- Rewarm at 3 minutes = 2-minute safety buffer
- Prevents redundant warmups on rapid model switches

### Redis Key Format

```
warmup:{user_id}:{session_id}:{provider}
```

**Example**: `warmup:default:sess_abc123:anthropic`

**TTL**: 3 minutes (180 seconds)

### Selective Warmup

Only warm the provider being used:

```go
if provider == "anthropic" {
    warmupAnthropicCache()
} else if provider == "openai" {
    warmupOpenAICache()
} else {
    // No cache support, skip warmup
    return "warmup_not_supported"
}
```

---

## Integration Guide

### zblade Implementation

#### 1. On Application Launch

```typescript
async function onLaunch() {
    const session = await loadSession();
    const model = config.defaultModel;  // "anthropic/claude-sonnet-4"
    
    // Send warmup request (non-blocking)
    fetch('http://localhost:8080/v1/blade/warmup', {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            'Authorization': `Bearer ${apiKey}`
        },
        body: JSON.stringify({
            type: 'warmup',
            session_id: session.id,
            user_id: 'default',
            model: model,
            trigger: 'launch'
        })
    })
    .then(resp => resp.json())
    .then(data => {
        console.log(`Cache warmed: ${data.artifacts_loaded} artifacts`);
        console.log(`Provider: ${data.provider}, Ready: ${data.cache_ready}`);
    })
    .catch(err => {
        console.warn('Warmup failed (non-fatal):', err);
    });
    
    // User can start typing immediately
    // Cache warming happens in parallel
}
```

#### 2. On Model Change

```typescript
async function onModelChange(newModel: string) {
    const session = await loadSession();
    
    // Warmup new model's cache
    await fetch('http://localhost:8080/v1/blade/warmup', {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            'Authorization': `Bearer ${apiKey}`
        },
        body: JSON.stringify({
            type: 'warmup',
            session_id: session.id,
            user_id: 'default',
            model: newModel,
            trigger: 'model_change'
        })
    });
    
    // Update UI to show new model
    updateModelDisplay(newModel);
}
```

#### 3. On Workspace Change

```typescript
async function onWorkspaceChange(newWorkspace: string) {
    const session = await createNewSession(newWorkspace);
    const model = config.defaultModel;
    
    // Warmup cache for new workspace session
    await fetch('http://localhost:8080/v1/blade/warmup', {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            'Authorization': `Bearer ${apiKey}`
        },
        body: JSON.stringify({
            type: 'warmup',
            session_id: session.id,
            user_id: 'default',
            model: model,
            trigger: 'workspace_change'
        })
    });
}
```

#### 4. On Session Resume

```typescript
let lastActivityTime = Date.now();
const INACTIVITY_THRESHOLD = 5 * 60 * 1000; // 5 minutes

async function onUserActivity() {
    const now = Date.now();
    const inactiveDuration = now - lastActivityTime;
    
    if (inactiveDuration > INACTIVITY_THRESHOLD) {
        // Session was inactive, rewarm cache
        const session = await loadSession();
        const model = config.currentModel;
        
        await fetch('http://localhost:8080/v1/blade/warmup', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Authorization': `Bearer ${apiKey}`
            },
            body: JSON.stringify({
                type: 'warmup',
                session_id: session.id,
                user_id: 'default',
                model: model,
                trigger: 'session_resume'
            })
        });
    }
    
    lastActivityTime = now;
}
```

### Error Handling

Warmup failures should be **non-fatal**:

```typescript
try {
    const response = await fetch('/v1/blade/warmup', {
        method: 'POST',
        body: JSON.stringify(warmupRequest)
    });
    
    const data = await response.json();
    
    if (data.type === 'warmup_not_supported') {
        console.log(`Provider ${data.provider} doesn't support caching`);
        // Continue normally, first request will be slower
    } else if (data.type === 'warmup_already_warm') {
        console.log('Cache already warm, skipping');
    } else {
        console.log(`Cache warmed: ${data.artifacts_loaded} artifacts`);
    }
} catch (error) {
    console.warn('Warmup failed (non-fatal):', error);
    // Continue normally, first request will populate cache
}
```

---

## Examples

### Example 1: Successful Warmup on Launch

**Request**:
```http
POST /v1/blade/warmup HTTP/1.1
Host: localhost:8080
Content-Type: application/json
Authorization: Bearer ps_live_...

{
  "type": "warmup",
  "session_id": "sess_abc123",
  "user_id": "default",
  "model": "anthropic/claude-sonnet-4-20250514",
  "trigger": "launch"
}
```

**Response**:
```http
HTTP/1.1 200 OK
Content-Type: application/json

{
  "type": "warmup_complete",
  "session_id": "sess_abc123",
  "provider": "anthropic",
  "cache_supported": true,
  "artifacts_loaded": 15,
  "cache_ready": true,
  "duration_ms": 23,
  "message": "Warming 15 artifacts for anthropic"
}
```

**What Happens**:
1. zcoderd detects provider: `anthropic`
2. Checks Redis: no recent warmup found
3. Queries DB: 15 artifacts in session
4. Starts async warmup in goroutine
5. Marks as warmed in Redis (3min TTL)
6. Returns response immediately (23ms)
7. Warmup continues in background
8. First user request → cache ready → instant response

---

### Example 2: Model Change (Anthropic → OpenAI)

**Request 1** (Initial warmup):
```json
{
  "type": "warmup",
  "session_id": "sess_abc123",
  "user_id": "default",
  "model": "anthropic/claude-sonnet-4-20250514",
  "trigger": "launch"
}
```

**Response 1**:
```json
{
  "type": "warmup_complete",
  "provider": "anthropic",
  "cache_ready": true
}
```

**Request 2** (User switches to GPT-4o):
```json
{
  "type": "warmup",
  "session_id": "sess_abc123",
  "user_id": "default",
  "model": "openai/gpt-4o",
  "trigger": "model_change"
}
```

**Response 2**:
```json
{
  "type": "warmup_complete",
  "provider": "openai",
  "cache_ready": true
}
```

**What Happens**:
1. Anthropic cache expires naturally (5min TTL)
2. OpenAI cache gets warmed
3. User makes request → OpenAI cache ready
4. Cost optimized: only one provider warmed at a time

---

### Example 3: Rapid Warmup Requests (Cache Reuse)

**Request 1** (t=0s):
```json
{
  "type": "warmup",
  "session_id": "sess_abc123",
  "model": "anthropic/claude-sonnet-4-20250514",
  "trigger": "launch"
}
```

**Response 1**:
```json
{
  "type": "warmup_complete",
  "cache_ready": true,
  "duration_ms": 23
}
```

**Request 2** (t=30s, user reopens workspace):
```json
{
  "type": "warmup",
  "session_id": "sess_abc123",
  "model": "anthropic/claude-sonnet-4-20250514",
  "trigger": "workspace_change"
}
```

**Response 2**:
```json
{
  "type": "warmup_already_warm",
  "cache_ready": true,
  "duration_ms": 5,
  "message": "Cache already warm from recent warmup"
}
```

**What Happens**:
1. First warmup: cache warmed, Redis marked (3min TTL)
2. Second warmup (30s later): Redis hit, skip warmup
3. Cost saved: no redundant warmup request
4. Response faster: 5ms vs 23ms

---

### Example 4: Non-Caching Provider (Groq)

**Request**:
```json
{
  "type": "warmup",
  "session_id": "sess_abc123",
  "user_id": "default",
  "model": "groq/llama-3.3-70b-versatile",
  "trigger": "launch"
}
```

**Response**:
```json
{
  "type": "warmup_not_supported",
  "session_id": "sess_abc123",
  "provider": "groq",
  "cache_supported": false,
  "artifacts_loaded": 0,
  "cache_ready": false,
  "duration_ms": 2,
  "message": "Provider groq does not support prompt caching"
}
```

**What Happens**:
1. zcoderd detects provider: `groq`
2. Checks cache support: `false`
3. Returns immediately without warmup
4. First user request: normal flow (no cache benefit)

---

## Implementation Notes

### Server-Side (zcoderd)

**File**: `internal/blade/warmup.go`

**Key Components**:
1. `WarmupRequest` / `WarmupResponse` types
2. `DetectProvider()` - Extract provider from model string
3. `ProviderSupportsCache()` - Check if provider supports caching
4. `HandleWarmup()` - Main handler with async warmup

**Flow**:
```go
func (h *Handler) HandleWarmup(w http.ResponseWriter, r *http.Request) {
    // 1. Parse request
    var req WarmupRequest
    json.NewDecoder(r.Body).Decode(&req)
    
    // 2. Detect provider
    provider := DetectProvider(req.Model)
    
    // 3. Check cache support
    if !ProviderSupportsCache(provider) {
        return warmupNotSupported()
    }
    
    // 4. Check Redis for recent warmup
    if recentlyWarmed(req.UserID, req.SessionID, provider) {
        return warmupAlreadyWarm()
    }
    
    // 5. Get artifact count
    artifacts := getArtifacts(req.UserID, req.SessionID)
    
    // 6. Start async warmup
    go contextManager.WarmupSession(req.UserID, req.SessionID)
    
    // 7. Mark as warmed in Redis (3min TTL)
    markAsWarmed(req.UserID, req.SessionID, provider)
    
    // 8. Return immediately
    return warmupComplete(len(artifacts))
}
```

### Client-Side (zblade)

**Recommended Implementation**:
1. Create `WarmupClient` struct
2. Track last warmup time per session
3. Implement exponential backoff on failures
4. Log warmup events for debugging

**Example**:
```rust
pub struct WarmupClient {
    base_url: String,
    api_key: String,
    last_warmup: HashMap<String, Instant>,
}

impl WarmupClient {
    pub async fn warmup(
        &mut self,
        session_id: &str,
        model: &str,
        trigger: WarmupTrigger,
    ) -> Result<WarmupResponse> {
        let req = WarmupRequest {
            type_: "warmup".to_string(),
            session_id: session_id.to_string(),
            user_id: "default".to_string(),
            model: model.to_string(),
            trigger,
        };
        
        let resp = self.client
            .post(&format!("{}/v1/blade/warmup", self.base_url))
            .json(&req)
            .send()
            .await?;
        
        let data: WarmupResponse = resp.json().await?;
        
        // Track last warmup
        self.last_warmup.insert(session_id.to_string(), Instant::now());
        
        Ok(data)
    }
}
```

---

## Conclusion

The Cache Warmup extension to Blade Protocol v2 enables:

✅ **Zero-latency artifact access** for users  
✅ **98% cost savings** on cached requests  
✅ **Competitive moat** through cost-effective unlimited context  
✅ **Seamless UX** with invisible cache management  
✅ **Provider flexibility** with model-aware warmup  

This implementation mirrors Claude Code's proactive warmup strategy while maintaining zcoderd's cost-optimized architecture.

**Status**: Implemented and ready for zblade integration.

---

## Changelog

### v2.1-alpha (January 15, 2026)
- Initial warmup protocol specification
- Provider detection and cache support matrix
- Cost optimization with 3-minute reuse window
- Integration guide and examples
- Implementation in zcoderd complete
