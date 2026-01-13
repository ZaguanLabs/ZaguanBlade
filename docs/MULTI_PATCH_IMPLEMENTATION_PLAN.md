# Multi-Patch Tool Implementation Plan

**Date**: 2026-01-09
**Status**: ✅ COMPLETE (All Phases Implemented)
**Priority**: High - Core Feature Evolution

## Overview

Upgrade the `apply_patch` tool from a single search-and-replace operation to a **multi-hunk atomic patching system** inspired by OpenAI Codex's `multi_replace_file_content`. This enables the AI to refactor entire files (imports, function signatures, logic) in a single tool call, dramatically improving reliability and speed.

---

## Current State Analysis

### Backend (`zcoderd`)

| Component | Location | Current Behavior |
|-----------|----------|------------------|
| Tool Definition | `internal/blade/tools.go:135-160` | Single `path`, `old_text`, `new_text` schema |
| Tool Description | `internal/blade/tools/apply_patch.txt` | Emphasizes "ONE apply_patch call" but schema only supports one replacement |
| System Prompts | `internal/prompts/gpt52.go`, `gui_agent.go`, etc. | Reference `apply_patch` for single edits |

### Frontend (`ZaguanBlade`)

| Component | Location | Current Behavior |
|-----------|----------|------------------|
| Change Type | `src/types/change.ts` | Supports `patch` (single old/new), `new_file`, `delete_file` |
| Proposal Listener | `src/hooks/useChat.ts:168-191` | Listens to `propose-changes` event, maps to `Change[]` |
| Tool Display | `src/components/ToolCallDisplay.tsx:64` | Shows "📝 Applying Code Changes" for `apply_patch` |
| Blade Protocol | `src/services/blade.ts` | Dispatches intents but doesn't handle multi-hunk diffs |

### Protocol Flow (Current)

```
AI → tool_call(apply_patch, {path, old_text, new_text})
    ↓
zcoderd → SSE event: tool_call
    ↓
zblade → propose-changes event with single Change
    ↓
Editor → Diff view (single hunk)
    ↓
User → Accept/Reject
    ↓
zblade → tool_result back to zcoderd
```

---

## Target Architecture

### New Tool Schema: `apply_patch` v2

```json
{
  "name": "apply_patch",
  "parameters": {
    "type": "object",
    "properties": {
      "path": {
        "type": "string",
        "description": "Path to the file to edit"
      },
      "patches": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "old_text": {
              "type": "string",
              "description": "Exact text to replace (must be unique within file)"
            },
            "new_text": {
              "type": "string",
              "description": "Replacement text"
            },
            "start_line": {
              "type": "integer",
              "description": "Optional: Line range hint for disambiguation"
            },
            "end_line": {
              "type": "integer",
              "description": "Optional: Line range hint for disambiguation"
            }
          },
          "required": ["old_text", "new_text"]
        },
        "minItems": 1,
        "description": "Array of replacements to apply atomically"
      }
    },
    "required": ["path", "patches"]
  }
}
```

### Backward Compatibility

The tool MUST support the legacy schema for existing sessions:

```json
// Legacy (still works)
{ "path": "...", "old_text": "...", "new_text": "..." }

// New (preferred)
{ "path": "...", "patches": [{ "old_text": "...", "new_text": "..." }, ...] }
```

Backend logic:
```go
if args.Patches != nil {
    // New multi-patch path
} else if args.OldText != "" {
    // Legacy single-patch path (wrap into single-element array)
    args.Patches = []Patch{{OldText: args.OldText, NewText: args.NewText}}
}
```

---

## Implementation Plan

### Phase 1: Backend (`zcoderd`) - **Core Engine**

#### 1.1 Schema Update
**File**: `internal/blade/tools.go`

- [x] Update `ToolDefinition` for `apply_patch` to include `patches` array
- [x] Keep `old_text`/`new_text` as optional for backward compat
- [x] Add `start_line`/`end_line` hint fields

#### 1.2 Tool Description Update
**File**: `internal/blade/tools/apply_patch.txt`

- [x] Document multi-hunk behavior
- [x] Explain atomicity guarantee
- [x] Add examples with 2-3 patches

#### 1.3 SSE Event Format Update
**File**: `internal/blade/streaming.go` (or new file)

When sending `tool_call` event for `apply_patch`, include full patches array:

```json
{
  "event": "tool_call",
  "data": {
    "id": "call_abc123",
    "name": "apply_patch",
    "arguments": {
      "path": "/src/main.go",
      "patches": [
        { "old_text": "func Old()", "new_text": "func New()" },
        { "old_text": "var X = 1", "new_text": "var X = 2" }
      ]
    }
  }
}
```

#### 1.4 Prompt Updates
**Files**: `internal/prompts/gpt52.go`, `gui_agent.go`, `anthropic.go`

- [x] Update tool instructions to encourage multi-hunk edits
- [x] Remove "DO NOT make multiple apply_patch calls" (now unnecessary)
- [ ] Add examples of batching imports + logic changes

---

### Phase 2: Frontend (`ZaguanBlade`) - **UI/UX**

#### 2.1 Type Updates
**File**: `src/types/change.ts`

```typescript
export type PatchHunk = {
  old_text: string;
  new_text: string;
  start_line?: number;
  end_line?: number;
};

export type Change =
  | {
      change_type: "multi_patch";
      id: string;
      path: string;
      patches: PatchHunk[];
    }
  | {
      change_type: "patch";  // Legacy single-patch
      id: string;
      path: string;
      old_content: string;
      new_content: string;
    }
  | { change_type: "new_file"; id: string; path: string; content: string; }
  | { change_type: "delete_file"; id: string; path: string; };
```

#### 2.2 Tool Call Parser
**File**: `src/hooks/useChat.ts` (or new utility)

- [x] Detect `apply_patch` tool calls
- [x] Parse `patches` array from arguments
- [x] Convert to `Change` objects with `change_type: "multi_patch"`
- [ ] Emit `propose-changes` event (handled by Tauri layer)

#### 2.3 Diff Visualization Component
**File**: NEW `src/components/MultiPatchDiff.tsx`

**Features**:
- [x] Render each hunk as a collapsible diff block
- [x] Show file path header with hunk count badge
- [x] Individual Accept/Reject per hunk OR Accept All / Reject All
- [x] Syntax highlighting for old (red) and new (green) code
- [x] Line number hints if provided

**UI Mockup**:
```
┌─────────────────────────────────────────────────────┐
│ 📝 /src/components/App.tsx  [3 changes]  ✓ All  ✗   │
├─────────────────────────────────────────────────────┤
│ ▼ Hunk 1 (lines 1-5)                                │
│   - import { useState } from 'react';               │
│   + import { useState, useEffect } from 'react';    │
│                                               ✓  ✗  │
├─────────────────────────────────────────────────────┤
│ ▶ Hunk 2 (lines 24-30)  [collapsed]           ✓  ✗  │
├─────────────────────────────────────────────────────┤
│ ▶ Hunk 3 (lines 88-95)  [collapsed]           ✓  ✗  │
└─────────────────────────────────────────────────────┘
```

#### 2.4 Editor Integration
**File**: `src/contexts/EditorContext.tsx` or CodeMirror extension

- [ ] When multi-patch is proposed, highlight ALL affected regions in editor
- [ ] Show inline "ghost text" for each hunk (green additions, strikethrough deletions)
- [ ] On Accept All: apply all patches atomically
- [ ] On partial accept: apply subset, return partial tool result

#### 2.5 Tool Result Handler
**File**: `src/hooks/useChat.ts`

- [ ] On Accept All: Send single `tool_result` with success for all hunks
- [ ] On partial accept: Send `tool_result` with list of applied/rejected hunks
- [ ] On Reject All: Send `tool_result` with error message

---

### Phase 3: Protocol & Atomicity

#### 3.1 Atomic Application Logic
**Location**: Likely in Tauri Rust layer or frontend TS

The patching algorithm must:
1. [x] **Pre-validate**: Check ALL `old_text` patterns exist and are unique BEFORE applying any
2. [x] **Compute offsets**: After applying patch N, calculate offset delta for patch N+1
3. [x] **Rollback on failure**: If any patch fails, restore original file content
4. [x] **Return granular status**: Which patches succeeded, which failed

```rust
// Pseudocode for Tauri command
fn apply_multi_patch(path: &str, patches: Vec<Patch>) -> Result<ApplyResult, Error> {
    let original = fs::read_to_string(path)?;
    let mut working = original.clone();
    let mut results = Vec::new();
    
    for patch in &patches {
        // Validate ALL patches first
        if working.matches(&patch.old_text).count() != 1 {
            return Err(Error::AmbiguousPatch(patch));
        }
    }
    
    // Apply in order
    for patch in patches {
        working = working.replacen(&patch.old_text, &patch.new_text, 1);
        results.push(PatchResult::Success);
    }
    
    fs::write(path, &working)?;
    Ok(ApplyResult { patches: results })
}
```

#### 3.2 Error Reporting
If patch `N` fails:
```json
{
  "success": false,
  "error": "Patch 2 failed: old_text not found",
  "applied": [0, 1],
  "failed": [2],
  "pending": [3, 4]
}
```

---

## Testing Strategy

### Unit Tests
- [ ] `zcoderd`: Test schema parsing for legacy and new formats
- [ ] `zcoderd`: Test SSE event emission with patches array
- [ ] `zblade`: Test Change type discrimination
- [ ] `zblade`: Test diff rendering for 1, 2, 5 hunks

### Integration Tests
- [ ] Full flow: AI calls multi-patch → UI renders → User accepts → File updated
- [ ] Partial accept: Accept 2 of 3 hunks
- [ ] Rollback: Patch 3 fails → file unchanged

### E2E Tests
- [ ] Use `gpt-5.2` to refactor a real file with multiple changes
- [ ] Verify all hunks appear in UI
- [ ] Verify file content after acceptance

---

## Migration Path

1. **Week 1**: Implement backend schema + SSE changes (backward compat)
2. **Week 2**: Implement frontend type + parser changes
3. **Week 3**: Build `MultiPatchDiff` component
4. **Week 4**: Integrate with editor, add ghost text preview
5. **Week 5**: Testing, polish, deploy

---

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| Breaking existing sessions | Backward compat in schema parser |
| Complex offset calculations | Pre-validate all patches, apply in order |
| UI clutter with many hunks | Collapsible hunks, "Accept All" button |
| Performance with large files | Lazy diff computation, virtual scroll |

---

## Success Metrics

- [ ] AI can refactor a 3-function file in ONE tool call (not 3)
- [ ] 95%+ of multi-patch operations complete without user intervention
- [ ] No regressions on single-patch workflows
- [ ] User feedback: "This is way faster than before"

---

## Appendix: Codex vs Anthropic Comparison

| Feature | Anthropic `Edit` | Codex `multi_replace` | Our Target |
|---------|-----------------|----------------------|------------|
| Batching | No (1 per call) | Yes (N per call) | Yes |
| Line hints | No | Yes | Yes (optional) |
| Atomicity | N/A | Full file | Full file |
| Uniqueness | Required | Required (with hints) | Required |
| Backward compat | N/A | N/A | Yes (legacy schema) |
