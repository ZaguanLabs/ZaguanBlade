# Zaguán Blade Security Audit

Date: 2026-04-09  
Auditor: GPT-5 via Zaguán Blade

## Executive Summary

I performed a source-level security audit of the Zaguán Blade codebase, focusing on trust boundaries between the React renderer, the Tauri backend, the terminal/command execution pipeline, filesystem access, and remote AI connectivity.

This audit found **multiple high-impact architectural weaknesses**. The most important theme is:

- the application grants the main renderer **very broad native capabilities**,
- while the main window runs with **`csp: null`**, and
- several privileged Tauri commands accept **caller-controlled paths or state** without binding them to the active workspace or a backend-side authorization policy.

That combination means a future renderer compromise, unsafe content rendering bug, plugin/API abuse, or other frontend injection issue would have a **very large blast radius**: arbitrary file read/write within the user home, command execution, state spoofing, and even forced application exit.

## Overall Risk Rating

**High**

Not because I found a turnkey remote exploit in one file, but because the current security posture creates a fragile trust model: the renderer is treated as highly trusted while also being connected to rich, untrusted inputs (LLM output, markdown, PDFs, WebSockets, file content, user workspaces).

## Methodology

This was a **static audit** of the repository. I reviewed:

- Tauri security and capability configuration
- backend command handlers under `src-tauri/src/commands/`
- terminal and command execution paths
- frontend markdown/document/PDF rendering paths
- WebSocket and remote AI transport code
- filesystem and project-state related commands

I did **not** perform live exploitation, fuzzing, dependency CVE scanning, or runtime packet capture.

## Positive Findings

There are also some good defensive patterns already present:

- Workspace file reads/writes in `src-tauri/src/commands/files.rs` are generally rooted and canonicalized through `resolve_path_under_workspace*`.
- Markdown rendering uses `react-markdown` without `rehypeRaw`, which materially reduces classic HTML-in-markdown XSS risk.
- `execute_native_command` executes a program plus args directly rather than always going through `sh -c`, which is safer than shell-string execution.
- Several command-approval paths exist for AI-triggered commands, which is better than fully automatic execution.

Those are meaningful strengths, but they are currently outweighed by the trust-boundary issues below.

---

## Findings

### 1. Disabled CSP plus broad native capabilities create a catastrophic renderer-compromise blast radius

**Severity:** High

The main Tauri window disables CSP entirely:

- `src-tauri/tauri.conf.json:26-28`

```json
"security": {
  "csp": null
}
```

At the same time, the default capability grants the renderer very broad access:

- `src-tauri/capabilities/default.json:8-29`

Notable permissions include:

- `shell:default`
- `fs:default`
- `fs:allow-read-file` for `$HOME/**`
- `fs:scope` for `$HOME/**`

This means any successful renderer-side compromise would have an unusually large local impact. Even if the current frontend rendering looks relatively safe today, the configuration sharply increases the consequences of:

- a future XSS bug,
- a malicious dependency,
- unsafe HTML/URL handling added later,
- compromised remote content,
- or a logic bug that lets attacker-controlled data reach privileged APIs.

**Impact**

A renderer compromise could plausibly lead to:

- reading arbitrary user files under `$HOME`,
- invoking Tauri commands directly,
- launching shell/plugin actions,
- manipulating application state and approvals,
- or chaining into local command execution.

**Recommendation**

- Re-enable a restrictive CSP for production.
- Split capabilities by window and feature area.
- Remove `shell:default` from the main renderer if possible.
- Narrow filesystem scope from `$HOME/**` to the active workspace plus specific app-owned directories.
- Treat the renderer as a semi-trusted client, not as the security boundary.

---

### 2. Privileged backend commands trust caller-supplied paths and app-control inputs too much

**Severity:** High

Several Tauri commands accept powerful inputs from the renderer without checking that the request is tied to the active workspace or otherwise authorized.

Examples:

- Arbitrary binary file read:
  - `src-tauri/src/commands/project.rs:77-82`
- Arbitrary project path operations:
  - `src-tauri/src/commands/project.rs:109-136`
- Renderer-selected local artifact/context roots:
  - `src-tauri/src/commands/local_context.rs:5-51`
  - `src-tauri/src/local_artifacts.rs:137-178`
- Arbitrary write path for ephemeral documents:
  - `src-tauri/src/ephemeral_commands.rs:62-78`
- Forced app shutdown from renderer:
  - `src-tauri/src/commands/project.rs:73`

Representative examples:

- `read_binary_file(path)` reads any supplied path.
- `save_ephemeral_document(id, path)` writes to any supplied path.
- `load_project_settings(project_path)`, `save_project_settings(project_path, ...)`, and `init_zblade_directory(project_path)` operate on any supplied path.
- `list_local_conversations(project_path)`, `load_local_conversation(project_path, ...)`, `search_local_moments(project_path, ...)`, `get_file_context(project_path, ...)`, and `delete_local_conversation(project_path, ...)` all instantiate `LocalArtifactStore::new(&path)` directly from a renderer-supplied path.
- `graceful_shutdown_with_state(...)` can terminate the app from the renderer.

These may be acceptable under a pure “the renderer is fully trusted” model, but that model is already weakened by the capability and CSP choices described above.

**Impact**

If the renderer is compromised, these commands substantially expand what an attacker can do without escaping into another process first:

- read arbitrary local files,
- write files outside the workspace,
- create or query `.zblade` metadata and artifact indexes in arbitrary directories selected by the renderer,
- modify `.gitignore` in arbitrary directories,
- or force app shutdown.

**Recommendation**

- For commands dealing with files or project paths, validate against the active workspace on the backend.
- Introduce explicit command-level authorization classes such as:
  - workspace-scoped,
  - app-config-scoped,
  - user-home-scoped,
  - privileged lifecycle.
- Remove or heavily restrict renderer access to `graceful_shutdown_with_state`.
- Avoid raw caller-controlled filesystem paths unless absolutely necessary.

---

### 3. Path traversal in `save_ephemeral_document_to_workspace` allows writes outside the workspace

**Severity:** High

`save_ephemeral_document_to_workspace` intends to save into the workspace root, but it constructs the destination path by joining the workspace root with an unsanitized filename:

- `src-tauri/src/ephemeral_commands.rs:101-126`

```rust
let file_path = workspace_root.join(&timestamped_filename);
fs::write(&file_path, &doc.content).await?;
```

There is **no validation** that `filename`:

- is only a basename,
- contains no `..`,
- contains no path separators,
- or resolves back under `workspace_root` after normalization.

Because the filename can include traversal segments, this can escape the workspace. The timestamping logic does not prevent traversal; it only changes the leaf name.

**Why this matters in this app**

Ephemeral documents are closely tied to AI-generated content and suggestions. If a suggested filename is ever model-controlled or attacker-influenced, a social-engineering path becomes realistic:

1. AI or untrusted content proposes/saves an ephemeral document with a crafted name.
2. User clicks Save.
3. File is written outside the workspace.

**Recommendation**

- Reject any filename containing `/`, `\\`, `..`, or platform separators.
- Accept basename-only values and reconstitute the final path server-side.
- Canonicalize or normalize and then verify the final path stays within `workspace_root` before writing.

---

### 4. Command-result submission trusts the renderer instead of the backend execution path

**Severity:** Medium

The backend accepts command completion via `submit_command_result(call_id, output, exit_code, ...)` based on a frontend-supplied `call_id` and status:

- `src-tauri/src/commands/tools.rs:1070-1113`

The function does not verify that:

- the result originated from the backend’s own command runner,
- the specific command is currently executing,
- the terminal/process identity matches,
- or the reported `exit_code`/`output` came from a real child process.

Instead, if the `call_id` matches a pending command, the result is accepted into workflow state.

**Impact**

A malicious or compromised renderer could potentially:

- mark a command as successful without it running,
- forge failure output to influence later model behavior,
- or interfere with the operator’s approval workflow.

This is not a direct remote code execution issue by itself, but it is a serious **integrity** problem in the AI command-execution pipeline.

**Recommendation**

- Keep authoritative command state in the backend.
- Bind command results to a backend-issued execution token or process handle.
- Accept completion only from the backend execution subsystem, not arbitrary renderer invocation.
- Consider making the frontend display-only for command streaming, not state-authoritative.

---

### 5. API keys are transmitted in WebSocket query strings and duplicated in payloads

**Severity:** Medium

The WebSocket client places the API key directly into the URL query string:

- `src-tauri/src/blade_ws_client.rs:300-305`

```rust
let url = format!("{}/v1/blade/v2?api_key={}", ws_url, self.api_key);
```

The key is also included in payloads for chat/tool-result flows:

- `src-tauri/src/blade_ws_client.rs:236-246`
- `src-tauri/src/blade_ws_client.rs:470-535`

Query-string credentials are generally weaker than header-based credentials because they are more likely to be exposed via:

- logs,
- proxies,
- telemetry,
- debugging tools,
- or error reporting.

**Impact**

This increases credential exposure risk and makes secret-handling harder to reason about and audit.

**Recommendation**

- Move WebSocket authentication to headers or an initial authenticated message only.
- Stop duplicating the API key in routine payloads once the session is authenticated.
- Redact secrets aggressively from logs and error surfaces.

---

### 6. Local-model connectivity features can be abused as a desktop SSRF / internal network probe surface

**Severity:** Medium

The local AI settings and test functions accept arbitrary URLs and use `reqwest` to connect to them:

- `src-tauri/src/commands/settings_local_ai.rs`
- `src-tauri/src/models/ollama.rs`
- `src-tauri/src/models/openai_compat.rs`

Examples include:

- `test_local_ollama_connection(ollama_url)`
- `test_local_openai_compat_connection(server_url)`
- model discovery requests to arbitrary configured endpoints

This behavior is part of the feature, so it is not inherently a bug. However, from a security perspective it means the desktop app can be turned into a network client toward arbitrary addresses if the renderer or configuration flow is compromised.

**Impact**

Potentially useful for:

- probing internal services,
- reaching local-only services from a compromised renderer,
- or leaking cloud API keys to unintended remote “local model” endpoints.

**Recommendation**

- Default these features to loopback-only (`localhost`, `127.0.0.1`, `::1`).
- Require an explicit warning/confirmation before allowing non-local endpoints.
- Distinguish “local” versus “remote custom endpoint” in both UI and policy.

---

### 7. Plaintext secret storage in config is a recoverability and local-exposure concern

**Severity:** Low / Medium

Remote and local AI credentials are stored in application config structures and persisted to disk:

- `src-tauri/src/config.rs`

This is common in desktop apps, but it means API keys are retrievable from local config files if the host is compromised, if backups are exposed, or if renderer-side leakage occurs.

**Recommendation**

- Prefer OS keychain/keyring storage for API keys.
- Keep only non-secret metadata in JSON config.
- Avoid returning secrets to the renderer unless strictly necessary.

---

## Additional Notes

### Markdown and document rendering

I did **not** find an obvious markdown-to-HTML XSS bug in the audited renderer components.

- `src/components/MarkdownRenderer.tsx` uses `react-markdown` without `rehypeRaw`.
- `src/components/DocumentViewer.tsx` also uses `react-markdown` with controlled code rendering.

That is good news. However, because CSP is disabled and capabilities are broad, even a single future regression in content rendering would be far more dangerous than normal.

### Workspace file confinement

The code in `src-tauri/src/commands/files.rs` shows a stronger pattern: workspace-root resolution and canonicalization are used for normal file operations. That should be treated as the standard and extended to the weaker commands noted above.

### Uncommitted-change rejection is snapshot-backed, not a raw path write

I also reviewed the AI uncommitted-change reject flow and the file-history revert flow:

- `src-tauri/src/commands/uncommitted.rs`
- `src-tauri/src/uncommitted_changes.rs`
- `src-tauri/src/history/mod.rs`
- `src-tauri/src/commands/history.rs`

Those paths do **not** directly take an arbitrary filesystem destination from the renderer at revert time. Rejection/revert resolves through backend-maintained snapshot metadata stored in the workspace-local `.zblade/history` area, and the actual write target comes from the history entry associated with the snapshot ID.

That is materially better than a direct `write(path_from_renderer, ...)` pattern, so I do **not** classify the reject/revert feature itself as an arbitrary-path-write vulnerability in this audit.

However, there is still an integrity concern worth tracking: `load_project_state(project_path)` repopulates backend `uncommitted_changes` from persisted project state chosen by a renderer-supplied project path. Likewise, the local-context commands (`list_local_conversations`, `load_local_conversation`, `search_local_moments`, `get_file_context`, `delete_local_conversation`) instantiate `LocalArtifactStore` directly from a renderer-provided `project_path`, so project-local artifact and context data are also anchored to caller-selected roots rather than a backend-authoritative active workspace. In practice, the blast radius appears more like cross-project metadata exposure/tampering than direct arbitrary file overwrite, but it still reflects the same broader pattern of backend state trusting renderer-selected project context more than it should.

## Priority Remediation Plan

### Immediate

1. Fix `save_ephemeral_document_to_workspace` path traversal.
2. Restrict or remove arbitrary-path commands from renderer access:
   - `read_binary_file`
   - `save_ephemeral_document`
   - project path commands using caller-supplied roots
   - `graceful_shutdown_with_state`
3. Stop sending API keys in WebSocket query strings.

### Short Term

4. Re-enable CSP for production.
5. Reduce Tauri permissions for the main window:
   - remove `shell:default` where possible,
   - narrow FS scope to workspace + app-owned dirs.
6. Make backend command execution authoritative; do not trust renderer-submitted command results.

### Medium Term

7. Introduce a formal permission model for backend commands.
8. Move secrets to OS-native secure storage.
9. Add security-focused tests for path validation and privilege boundaries.

## Suggested Test Cases to Add

- Reject `../` and absolute paths in `save_ephemeral_document_to_workspace`.
- Verify `read_binary_file` cannot read outside the active workspace unless explicitly intended.
- Verify `save_project_settings`, `init_zblade_directory`, and local-context commands reject unrelated project roots.
- Verify `submit_command_result` is ignored unless tied to an active backend execution token.
- Verify non-local “local AI” URLs trigger explicit warnings or policy rejection.

## Conclusion

Zaguán Blade already has some solid foundations in workspace path handling and markdown rendering, but the current desktop trust model is too permissive. The most serious risk is not one isolated parsing bug; it is the combination of:

- disabled CSP,
- broad renderer-native permissions,
- and backend commands that still trust renderer-supplied paths and execution state.

If you want, the next best follow-up would be a **remediation patch set** that fixes the highest-risk issues first:

1. filename/path confinement for ephemeral saves,  
2. backend restriction of arbitrary-path project commands, and  
3. removal of API keys from WebSocket URLs.
