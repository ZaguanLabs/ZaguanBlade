# TODO List - January 3, 2026

## High Priority
1. Implement database encryption for sensitive conversation data
- [x] Encrypt conversation transcripts, message content, API keys
- [x] Use AES-256-GCM for encryption
- [x] Secure key management via environment variables

2. Implement context compression with reference creation and injection
- [x] Trigger compression at message/token thresholds (20 messages)
- [x] Generate summaries via compression model (Kimi K2)
- [x] Create context_nodes in DB
- [x] Inject references into system prompt
- [x] Artifact catalog + retrieval tools (artifact_search/artifact_list/artifact_get + @artifact refs + prompt guidance)
- [x] Persist direct @search/@web/@research results as artifacts (project-scoped)
- [x] Redis short-term cache (fast reconnection)
- [x] **Permanent DB storage for ALL messages** (infinite persistence)
- [x] Clear Redis after compression
- [x] Save compressed segments to DB + Git repository
- [x] Multi-day conversation continuity (DB restore)
- [ ] File change tracking (staleness detection)
- [ ] Semantic retrieval (vector embeddings)
- [ ] Enhanced artifact extraction

3. Fix compression 401 authentication error
- [ ] Investigate why compression job fails with 401 unauthorized
- [ ] Verify compression model API key is properly configured
- [ ] Check if auth header is being passed correctly to compression endpoint
- [ ] Add proper error logging to identify exact failure point

4. Implement Skills/Plugins system (inspired by Claude Code)
- [ ] Design skill format: Markdown files with YAML frontmatter (name, description, auto-invoke conditions)
- [ ] Create skills directory structure in zcoderd (e.g., `skills/` folder)
- [ ] Implement skill detection: analyze user request to determine which skills are relevant
- [ ] Implement skill injection: dynamically add relevant skill content to system prompt
- [ ] Support multiple skills per request when appropriate
- [ ] Create initial skills library:
  - [ ] Frontend design (bold aesthetics, avoiding generic AI patterns)
  - [ ] Security guidance (warn about common vulnerabilities)
  - [ ] Code review (structured review approach)
  - [ ] Bug fixing (systematic debugging methodology)
  - [ ] Performance optimization (profiling and optimization strategies)
- [ ] Add skill management commands (list, enable/disable, custom skills)
- [ ] Document skill creation format and best practices
- [ ] Consider skill marketplace/sharing mechanism (future)

5. Implement "Full Context" fast-loading tool (Windsurf-style Fast Context)
- [ ] Design a one-shot context loader that streams a curated snapshot of the repo (file tree + top-N key files + heuristically chosen config/entrypoints) to the model in a single call for faster, higher-confidence answers.
- [ ] Heuristics: prioritize entrypoints (main.rs, lib.rs, src/**/mod.rs), configs (Cargo.toml, package.json, tsconfig), infra (Dockerfile, CI), and recently-changed files; include sizes/paths to avoid overloading tokens.
- [ ] Transport: gzip-compressed payload with paging/chunking and size guards; fall back to multi-call when payload exceeds limits.
- [ ] Server support: add a backend endpoint/tool to assemble the snapshot efficiently (streaming read + size budget, skip binaries/large assets).
- [ ] Client support: UI control to trigger Fast Context; show progress; cache latest snapshot per workspace/session.
- [ ] Safety: enforce workspace boundary, ignore gitignored/binaries by default, allow include/exclude globs.
- [ ] Benchmark vs. incremental single-file tools: latency, token cost, accuracy; tune heuristics accordingly.

## Medium Priority
3. Add TODO list system prompt instructions for AI models in zcoderd
- [ ] Instruct models to create and maintain task lists
- [ ] Format: [DONE], [IN_PROGRESS], [PENDING]
- [ ] Show list at natural checkpoints

4. Redesign /init into "project bootstrap snapshot" (client-driven, likely in ZaguanEditor)
- [ ] Locate discarded/initial implementation in ZaguanEditor
- [ ] Re-spec as long-running job: scan project, extract entrypoints/config, create initial artifacts
- [ ] Persist snapshot artifacts to DB + Git (project-scoped)
- [ ] Provide progress updates and resumability

5. Improve WebSocket capabilities (beyond basic streaming)
- [ ] Add resumable streams (replay from sequence)
- [ ] Multiplex background jobs (compression/indexing/init) over a single connection
- [ ] Server-push progress events + unified SSE/WS event model

6. Add chat history tab showing saved chats from DB
- [ ] New tab in chat pane
- [ ] List sessions from DB
- [ ] Click to restore conversations

7. Implement chat titles stored in DB and displayed in UI
- [ ] Add title column to sessions table
- [ ] Auto-generate from first message or AI summary
- [ ] Display in history list

8. Research Alacritty integration for zblade terminal
- [ ] Investigate Alacritty as embedded terminal option (native Rust, should be more stable)
- [ ] Compare with current xterm.js implementation (flickering issues)
- [ ] Evaluate performance and stability benefits (native vs web-based)
- [ ] Assess integration complexity with Tauri (both Rust-based, potentially natural fit)
- [ ] Research embedding strategies (separate window, webview bridge, or native widget)
- [ ] Consider trade-offs: implementation effort vs stability gains

9. Refactor zblade lib.rs into logical modules
- [ ] Analyze current lib.rs structure and identify logical groupings
- [ ] Extract approval flow logic into separate module
- [ ] Extract command execution logic into separate module
- [ ] Extract tool result handling into separate module
- [ ] Ensure clean module boundaries and minimal coupling

10. Fix garbled responses from Qwen3 model
- [ ] Investigate why Qwen3 responses are malformed (missing spaces, concatenated words)
- [ ] Check if issue is in token decoding, streaming, or model output
- [ ] Test with other models to isolate if it's Qwen3-specific
- [ ] Review CoreX/zcoderd streaming implementation for Alibaba provider
- [ ] Consider if it's a model temperature/sampling parameter issue

## Low Priority (Time Permitting)
8. Customize CodeMirror editor
- [ ] Theme tweaks (colors, fonts, spacing)
- [ ] Custom keybindings
- [ ] Enhanced syntax highlighting

9. Review and improve file explorer
- [ ] UI/UX improvements
- [ ] Better file icons
- [ ] Context menu enhancements

