# AI-LSP Integration: The "Magic" Feature

## Overview

This is zblade's **killer feature**: LSP diagnostics automatically feed into the AI, enabling proactive error correction without user intervention.

**What Windsurf does:**
- LSP detects errors/warnings
- Sends diagnostics to AI
- AI analyzes and proposes fixes
- User accepts/rejects fixes

**zblade's advantage:** We control both the LSP layer AND the AI layer, enabling deeper integration.

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    LSP Server                            │
│  (rust-analyzer, gopls, etc.)                           │
└─────────────────────────────────────────────────────────┘
                          ↓
              textDocument/publishDiagnostics
                          ↓
┌─────────────────────────────────────────────────────────┐
│                zblade Backend (Rust)                     │
│  ┌────────────────────────────────────────────────────┐ │
│  │  LSP Manager                                       │ │
│  │  - Receives diagnostics                            │ │
│  │  - Filters relevant errors                         │ │
│  └────────────────────────────────────────────────────┘ │
│                          ↓                               │
│  ┌────────────────────────────────────────────────────┐ │
│  │  AI Diagnostic Analyzer                            │ │
│  │  - Analyzes error context                          │ │
│  │  - Determines if AI can fix                        │ │
│  │  - Builds fix request                              │ │
│  └────────────────────────────────────────────────────┘ │
│                          ↓                               │
│  ┌────────────────────────────────────────────────────┐ │
│  │  AI Workflow Manager                               │ │
│  │  - Sends to zcoderd                                │ │
│  │  - Receives AI fix                                 │ │
│  │  - Proposes edit                                   │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
                          ↓
                    propose-edit event
                          ↓
┌─────────────────────────────────────────────────────────┐
│                zblade Frontend (React)                   │
│  - Shows diagnostic inline                              │
│  - Shows AI-proposed fix                                │
│  - User accepts/rejects                                 │
└─────────────────────────────────────────────────────────┘
```

---

## Event Flow

### 1. LSP Detects Error

```rust
// LSP sends diagnostic
{
  "uri": "file:///workspace/src/main.rs",
  "diagnostics": [{
    "range": {
      "start": { "line": 10, "character": 5 },
      "end": { "line": 10, "character": 15 }
    },
    "severity": 1,  // Error
    "message": "cannot find value `foo` in this scope",
    "code": "E0425"
  }]
}
```

### 2. Backend Analyzes Diagnostic

```rust
// src-tauri/src/ai_diagnostic_analyzer.rs
pub struct DiagnosticAnalyzer {
    // Determines if diagnostic is AI-fixable
}

impl DiagnosticAnalyzer {
    pub fn should_request_fix(&self, diagnostic: &Diagnostic) -> bool {
        // Only auto-fix certain error types
        match diagnostic.severity {
            DiagnosticSeverity::Error => {
                // Check if it's a fixable error type
                self.is_fixable_error(&diagnostic.code)
            },
            DiagnosticSeverity::Warning => {
                // Maybe fix warnings based on user settings
                self.user_wants_warning_fixes()
            },
            _ => false
        }
    }
    
    fn is_fixable_error(&self, code: &str) -> bool {
        // Rust errors that AI can typically fix
        matches!(code,
            "E0425" |  // Cannot find value
            "E0433" |  // Failed to resolve import
            "E0308" |  // Mismatched types
            "E0599" |  // No method found
            "E0061" |  // Wrong number of arguments
            // ... more fixable errors
        )
    }
}
```

### 3. Build AI Fix Request

```rust
pub async fn request_ai_fix(
    diagnostic: &Diagnostic,
    file_path: &str,
    file_content: &str,
) -> Result<(), String> {
    // Get surrounding context
    let context = extract_context(file_content, diagnostic.range);
    
    // Build prompt
    let prompt = format!(
        "Fix this error:\n\
         File: {}\n\
         Error: {}\n\
         Code: {:?}\n\n\
         Context:\n{}\n\n\
         Provide only the corrected code.",
        file_path,
        diagnostic.message,
        diagnostic.code,
        context
    );
    
    // Send to AI workflow
    let ai_manager = get_ai_manager();
    ai_manager.request_fix(prompt, file_path, diagnostic.range).await
}
```

### 4. AI Responds with Fix

```rust
// AI returns fixed code
// Backend emits propose-edit event
app.emit("propose-edit", ProposeEditPayload {
    id: generate_id(),
    path: file_path.to_string(),
    old_content: original_code,
    new_content: ai_fixed_code,
    reason: Some(format!("AI fix for: {}", diagnostic.message)),
})?;
```

### 5. Frontend Shows Fix

```typescript
// User sees:
// ❌ Error: cannot find value `foo` in this scope
// 💡 AI suggests: Did you mean `bar`?
// [Accept] [Reject] [Modify]
```

---

## Smart Features

### 1. **Contextual Fixes**

AI gets full context:
- Error message
- Error code
- Surrounding code
- File type
- Project structure (via workspace context)

### 2. **Batch Fixes**

```rust
// Fix multiple related errors at once
pub async fn batch_fix_diagnostics(
    diagnostics: Vec<Diagnostic>,
    file_path: &str,
) -> Result<(), String> {
    // Group related errors
    let groups = group_related_diagnostics(diagnostics);
    
    for group in groups {
        // Request single fix for all related errors
        request_ai_fix_batch(group, file_path).await?;
    }
}
```

**Example:**
```rust
// Multiple "unused import" warnings
use std::fs;
use std::io;
use std::path::Path;  // unused

// AI removes all unused imports in one fix
```

### 3. **Learning from Rejections**

```rust
pub struct FixHistory {
    accepted: Vec<FixRecord>,
    rejected: Vec<FixRecord>,
}

impl FixHistory {
    pub fn should_suggest_fix(&self, diagnostic: &Diagnostic) -> bool {
        // Don't suggest if user rejected similar fix before
        !self.was_recently_rejected(diagnostic)
    }
}
```

### 4. **Confidence Levels**

```rust
pub enum FixConfidence {
    High,    // Auto-apply (with user setting)
    Medium,  // Show suggestion
    Low,     // Don't suggest
}

pub fn calculate_confidence(diagnostic: &Diagnostic) -> FixConfidence {
    match diagnostic.code {
        "E0433" => FixConfidence::High,  // Missing import - easy fix
        "E0308" => FixConfidence::Medium, // Type mismatch - needs review
        "E0277" => FixConfidence::Low,    // Trait not implemented - complex
        _ => FixConfidence::Medium,
    }
}
```

---

## User Settings

```json
// ~/.zblade/settings.json
{
  "ai": {
    "autoFix": {
      "enabled": true,
      "autoApplyHighConfidence": false,  // Still require user approval
      "fixErrors": true,
      "fixWarnings": false,
      "fixHints": false,
      "excludeErrorCodes": ["E0277"],  // Don't auto-fix these
      "batchRelatedFixes": true
    }
  }
}
```

---

## Events

### New Events for AI-LSP Integration

```rust
// src-tauri/src/events.rs

// LSP diagnostic received
pub const DIAGNOSTIC_RECEIVED: &str = "diagnostic-received";

#[derive(Serialize)]
pub struct DiagnosticReceivedPayload {
    pub file_path: String,
    pub diagnostics: Vec<Diagnostic>,
}

// AI analyzing diagnostic
pub const AI_ANALYZING_DIAGNOSTIC: &str = "ai-analyzing-diagnostic";

#[derive(Serialize)]
pub struct AiAnalyzingDiagnosticPayload {
    pub file_path: String,
    pub diagnostic_message: String,
}

// AI fix proposed (uses existing propose-edit)
// User can see it came from AI via the `reason` field

// AI fix applied
pub const AI_FIX_APPLIED: &str = "ai-fix-applied";

#[derive(Serialize)]
pub struct AiFixAppliedPayload {
    pub file_path: String,
    pub diagnostic_message: String,
    pub fix_confidence: String,
}

// AI fix rejected
pub const AI_FIX_REJECTED: &str = "ai-fix-rejected";

#[derive(Serialize)]
pub struct AiFixRejectedPayload {
    pub file_path: String,
    pub diagnostic_message: String,
    pub reason: Option<String>,
}
```

---

## Implementation Phases

### Phase 1: Basic Integration (Week 1-2)

**Features:**
1. ✅ LSP diagnostics trigger AI analysis
2. ✅ AI proposes fixes for simple errors
3. ✅ User approves/rejects fixes
4. ✅ Track fix success rate

**Error Types:**
- Missing imports
- Undefined variables (typos)
- Unused variables/imports

### Phase 2: Smart Fixes (Week 3-4)

**Features:**
5. ✅ Batch related fixes
6. ✅ Confidence scoring
7. ✅ Context-aware fixes
8. ✅ Learning from rejections

**Error Types:**
- Type mismatches
- Missing method implementations
- Wrong argument counts

### Phase 3: Advanced (Week 5-6)

**Features:**
9. ✅ Auto-apply high-confidence fixes (opt-in)
10. ✅ Fix suggestions in hover
11. ✅ "Fix all similar" action
12. ✅ AI explains why fix works

**Error Types:**
- Complex type errors
- Lifetime issues (Rust)
- Async/await problems

### Phase 4: Intelligence (Week 7-8)

**Features:**
13. ✅ Predict errors before LSP reports them
14. ✅ Suggest refactorings to prevent errors
15. ✅ Learn project-specific patterns
16. ✅ Multi-file fixes

---

## UI/UX

### Inline Diagnostic with AI Fix

```
10 | let result = foo + bar;
   |              ^^^ cannot find value `foo` in this scope
   |
   | 💡 AI suggests: Did you mean `self.foo`?
   | [Accept] [Reject] [Show Diff]
```

### Notification

```
┌────────────────────────────────────────┐
│ 🤖 AI Fixed 3 Errors                   │
│                                        │
│ • Added missing import: use std::fs   │
│ • Fixed typo: foo → self.foo          │
│ • Removed unused variable: temp       │
│                                        │
│ [View Changes] [Undo All]             │
└────────────────────────────────────────┘
```

### Settings Panel

```
┌─────────────────────────────────────────┐
│ AI Auto-Fix Settings                    │
├─────────────────────────────────────────┤
│ ☑ Enable AI error fixing                │
│ ☐ Auto-apply high-confidence fixes      │
│                                         │
│ Fix these diagnostic types:             │
│ ☑ Errors                                │
│ ☐ Warnings                              │
│ ☐ Hints                                 │
│                                         │
│ Excluded error codes:                   │
│ [E0277, E0495]                          │
│                                         │
│ ☑ Batch related fixes                   │
│ ☑ Learn from my rejections              │
└─────────────────────────────────────────┘
```

---

## Performance Considerations

### 1. **Debouncing**

```rust
// Don't spam AI on every keystroke
pub struct DiagnosticDebouncer {
    pending: HashMap<String, Vec<Diagnostic>>,
    timer: Timer,
}

impl DiagnosticDebouncer {
    pub fn add_diagnostic(&mut self, file: String, diagnostic: Diagnostic) {
        self.pending.entry(file).or_default().push(diagnostic);
        self.reset_timer();  // Wait 2 seconds of no new diagnostics
    }
    
    async fn on_timer_expired(&mut self) {
        // Now send to AI
        for (file, diagnostics) in self.pending.drain() {
            analyze_diagnostics(file, diagnostics).await;
        }
    }
}
```

### 2. **Caching**

```rust
pub struct FixCache {
    // Cache AI fixes for identical errors
    cache: HashMap<DiagnosticKey, String>,
}

#[derive(Hash, Eq, PartialEq)]
struct DiagnosticKey {
    code: String,
    message: String,
    context_hash: u64,  // Hash of surrounding code
}
```

### 3. **Rate Limiting**

```rust
// Don't overwhelm zcoderd
pub struct RateLimiter {
    max_requests_per_minute: usize,
    current_count: usize,
}
```

---

## Privacy & Security

### 1. **User Consent**

- First-time setup: "Allow AI to analyze errors?"
- Clear explanation of what's sent to AI
- Opt-out anytime

### 2. **Data Sent to AI**

**Sent:**
- Error message
- Error code
- Surrounding code (5-10 lines)
- File type

**NOT sent:**
- Full file contents (unless needed)
- Other files in workspace
- Secrets/credentials (filtered)

### 3. **Secret Detection**

```rust
pub fn sanitize_context(code: &str) -> String {
    // Remove potential secrets before sending to AI
    let patterns = [
        r"password\s*=\s*['\"].*['\"]",
        r"api_key\s*=\s*['\"].*['\"]",
        r"token\s*=\s*['\"].*['\"]",
    ];
    
    let mut sanitized = code.to_string();
    for pattern in patterns {
        sanitized = Regex::new(pattern).unwrap()
            .replace_all(&sanitized, "$1 = \"***\"")
            .to_string();
    }
    sanitized
}
```

---

## Metrics & Analytics

Track to improve AI fixes:

```rust
pub struct FixMetrics {
    pub total_diagnostics: usize,
    pub ai_fixes_proposed: usize,
    pub ai_fixes_accepted: usize,
    pub ai_fixes_rejected: usize,
    pub fix_latency_ms: Vec<u64>,
    pub by_error_code: HashMap<String, ErrorCodeMetrics>,
}

pub struct ErrorCodeMetrics {
    pub count: usize,
    pub fix_success_rate: f64,
    pub avg_confidence: f64,
}
```

**Display in UI:**
```
AI Fix Statistics (Last 7 Days)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Diagnostics analyzed: 247
Fixes proposed: 189 (76%)
Fixes accepted: 156 (83% acceptance)
Avg response time: 1.2s

Top Fixed Errors:
1. E0433 (Missing import) - 98% success
2. E0425 (Undefined value) - 87% success
3. E0308 (Type mismatch) - 72% success
```

---

## Competitive Advantages

**vs Windsurf:**
- ✅ Tighter integration (we control both LSP and AI)
- ✅ Batch fixes (fix multiple related errors)
- ✅ Learning from user preferences
- ✅ Project-specific patterns
- ✅ Offline mode (cache common fixes)

**vs GitHub Copilot:**
- ✅ Error-driven (not just completion)
- ✅ Context from LSP (type info, diagnostics)
- ✅ Whole-project understanding
- ✅ Fix verification (LSP confirms fix works)

**vs Cursor:**
- ✅ Open source AI backend (zcoderd)
- ✅ Full control over prompts
- ✅ Custom fix strategies per language
- ✅ Privacy (self-hosted option)

---

## Future Enhancements

### 1. **Predictive Fixes**

```rust
// Predict errors before LSP reports them
pub async fn predict_errors(code: &str) -> Vec<PredictedError> {
    // Use AI to analyze code patterns
    // Suggest fixes before compilation
}
```

### 2. **Multi-File Fixes**

```rust
// Fix errors that span multiple files
pub async fn fix_across_files(diagnostic: &Diagnostic) -> Vec<FileEdit> {
    // Example: Add import in one file, export in another
}
```

### 3. **Refactoring Suggestions**

```rust
// Suggest refactorings to prevent future errors
pub async fn suggest_refactoring(file: &str) -> Vec<Refactoring> {
    // "This function is too complex, consider splitting it"
    // "These errors suggest you need a new abstraction"
}
```

### 4. **Test Generation**

```rust
// Generate tests that would catch the error
pub async fn generate_test_for_error(diagnostic: &Diagnostic) -> String {
    // Create regression test
}
```

---

## Summary

**The Magic:**
LSP → Diagnostics → AI → Fixes → User Approval → Applied

**The Value:**
- Faster development (errors fixed automatically)
- Learning tool (see how AI fixes errors)
- Reduced frustration (no more googling error codes)
- Better code quality (consistent fixes)

**The Differentiator:**
zblade doesn't just show errors - it **fixes them intelligently**.

This is what makes zblade more than just an editor. It's an **AI pair programmer** that actively helps you write better code.
