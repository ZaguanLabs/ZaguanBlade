# Codex System Prompt & Responses API Analysis

## Overview
This document analyzes the internal configuration and protocols of the OpenAI Codex CLI (`codex-rs`), focusing on the `gpt-5.2` system prompt and the `Responses` API format.

## 1. System Prompts (`models.json`)
The core system instructions are defined in `codex-rs/core/models.json`. This file contains the configuration for various models, including `gpt-5.1-codex`, `gpt-5.2`, and `gpt-5`.

### GPT-5.2 Base Instructions
The `gpt-5.2` model uses a highly detailed system prompt that emphasizes:

*   **Personality**: Concise, direct, friendly. "No 'AI slop' or safe, average-looking layouts."
*   **AGENTS.md Protocol**: The agent is explicitly instructed to look for and obey `AGENTS.md` files in the repository. These files can contain project-specific coding conventions or architectural guidelines.
    *   *Scope*: Recursive from root to CWD.
    *   *Precedence*: User prompt > Deepest `AGENTS.md` > Root `AGENTS.md`.
*   **Ambition vs. Precision**:
    *   *New Projects*: Be ambitious and creative.
    *   *Existing Codebases*: Be surgically precise; do not change style or conventions unnecessarily.
*   **Planning**: strict rules for using the `update_plan` tool.
    *   Plans must be high-quality (logical phases, not just "write code").
    *   One `in_progress` item at a time.
*   **Formatting Rules (Enforced)**:
    *   Tiny changes (<=10 lines): 2-5 sentences, no headers.
    *   Medium changes: <=6 bullets.
    *   Large changes: Summarize per file.
    *   **NEVER** use "before/after" blocks or dump full files.
*   **Tooling**:
    *   Prefer `apply_patch` (custom format) over writing full files.
    *   Prefer `rg` over `grep`.

## 2. Responses API Protocol (`openai_models.rs` & `models.rs`)

The "Responses" API (`/v1/responses`) represents a structural evolution from the Chat Completions API.

### Key Types

#### `ResponseInputItem` (Request)
What the client sends to the model:
*   **`Message`**: Standard content.
*   **`FunctionCallOutput`**:
    *   `call_id`: standard.
    *   `output`: **`FunctionCallOutputPayload`** (See below).

#### `ResponseItem` (Response)
What the model streams back:
*   **`Message`**: Standard text delta.
*   **`Reasoning`**: A distinct stream for "thinking" or CoT (Chain of Thought).
    *   Contains `summary` and `content`.
    *   Can be `encrypted_content` (likely for proprietary reasoning models).
*   **`FunctionCall`**: Standard tool calls.
*   **`LocalShellCall`**: A dedicated type for shell commands, distinct from generic function calls.
    *   Status: `Completed`, `InProgress`.
    *   Action: `Exec` (command, timeout, workdir, env, user).
*   **`WebSearchCall`**: Built-in support for browser actions.
    *   Actions: `Search`, `OpenPage`, `FindInPage`.
    *   Status: `open`, `in_progress`, `completed`.

#### `FunctionCallOutputPayload`
The structure of tool results is stricter:
```rust
pub struct FunctionCallOutputPayload {
    pub content: String, // Legacy string path
    pub content_items: Option<Vec<FunctionCallOutputContentItem>>, // Structured content (Text or Image)
    pub success: Option<bool>, // EXPLICIT success flag
}
```
*   **Success Flag**: The API expects to know if a tool call succeeded (`true`) or failed (`false`). This drives the model's error correction behavior.
*   **Structured Content**: Tool outputs can be mixed Text and Image/Multimodal.

## 3. Tool Definitions
*   **`apply_patch`**: A custom, "stripped-down file-oriented diff format".
    *   Headers: `*** Add File`, `*** Delete File`, `*** Update File`.
    *   No JSON wrapping for the patch content itself (it's a string argument).
*   **`shell`**:
    *   Standard `command`, `workdir`, `timeout`.
    *   **`sandbox_permissions`**: `use_default` or `require_escalated`.
    *   **`justification`**: Required when requesting escalation.

## Insights & Actions for Zaguán
1.  **Strict Success/Failure**: [COMPLETED] We have updated `ResponseItem` in `responses.go` to use `interface{}` for Output and defined `FunctionCallOutputPayload` with the required `Success` boolean. `ConvertToResponses` now wraps tool results in this structured format.
2.  **Reasoning Streams**: [COMPLETED] We have implemented `streaming.go` support for the Responses API stream format, parsing `response.output_item.added`, `response.text.delta` etc. correctly.
3.  **AGENTS.md**: [DEFERRED] Feature identified but implementation deferred. Prompt changes reverted.
4.  **Web Search**: [STATUS: Mapped] We are mapping `search_web` as a standard function tool.
5.  **Critical Fix**: [COMPLETED] Fixed `streaming.go` to correctly parse `ResponsesStreamEvent` structures instead of generic chunks.
