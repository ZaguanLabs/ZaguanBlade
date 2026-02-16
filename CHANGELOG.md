# Changelog

All notable changes to Zaguán Blade will be documented in this file.

## [0.2.0-alpha] - 2025-02-15

### Architecture
- **Provider runtime abstraction** — unified streaming architecture across all AI providers (Ollama, OpenAI-compat, cloud)
- **Ollama cloud authentication** — added support for authenticated Ollama cloud endpoints
- **gix integration** — replaced CLI-based git operations with native Rust `gix` for remote URL, log, and commit preflight (faster, no subprocess overhead)

### Bug Fixes
- **Reasoning card persistence** — reasoning/chain-of-thought blocks no longer disappear when the model transitions from reasoning to final answer
- **Terminal echo corruption** — fixed `run_command` leaking sentinel fragments (`( echo '`) into terminal output after repeated use
- **`read_file_range` segfault** — clamped line-range bounds to prevent out-of-bounds panics when requested range exceeds file length
- **Diff view content visibility** — fixed critical bug where added lines (green highlights) were not displayed in the diff view
- **Editor file reload** — robustly match file paths for external change events so the active editor reliably reloads after tool edits and filesystem updates
- **Command Center caret drift** — fixed Shift+Enter visual cursor glitch where the caret would appear stuck after inserting a newline
- **Command Center input lag** — eliminated per-keystroke layout thrash by coalescing textarea autosize into animation-frame batches
- **GLM model image guard** — prevent image attachments for GLM models that don't support them

### UI/UX Improvements
- **Sidebar overlap fix** — floating sidebar no longer casts a shadow or intercepts pointer events when hidden
- **Resize cursors** — chat/terminal pane dividers now show proper `col-resize` / `row-resize` cursors with larger hit areas
- **Chat autoscroll** — natural scroll-follow during streaming that respects manual scroll-away
- **Model selector** — memoized derived lists and removed delayed smooth-scroll for snappier open/close
- **Settings save** — modal closes immediately; persistence continues in background
- **History titles** — robust fallback title generation when backend title is empty
- **Title bar** — modernized window controls with tighter spacing, rounded buttons, and cleaner hover/active states
- **Terminal spacing** — increased left-side padding to avoid visual clash with floating sidebar
- **Reduced launch flicker** — removed extra startup delays; loading overlay fades out on first React frame
- **Git panel** — improved layout, application icon in title bar, push indicator badge
- **Chat message blocks** — preserved block ordering and improved block merging logic

### Chores
- Bumped version to 0.2.0-alpha
- Updated application icons
- Simplified development setup documentation
- Removed stale `.zblade` configuration directory and `scap` submodule

## [0.1.1] - 2025-02-09

- CI: Added `libpipewire-0.3-dev`, `libgbm-dev`, `libegl-dev`, `libxcb1-dev` for xcap linking on Ubuntu
- CI: Upgraded Ubuntu runner from 22.04 to 24.04
- Git push success feedback in GitPanel

## [0.1.0] - 2025-02-08

- Initial release
