# .gitignore Support in zblade/zcoderd

## Overview

zblade and zcoderd now support respecting `.gitignore` patterns when performing file operations. This prevents the AI from accessing sensitive files (like `.env`, `node_modules/`, build artifacts, etc.) unless explicitly allowed by the user.

## Architecture

### Default Behavior
- **Default**: `.gitignore` patterns are **respected** (gitignored files are filtered out)
- **User can override**: Setting `allow_gitignored_files: true` grants full access

This matches the behavior of modern AI coding assistants like Windsurf and Cursor.

### Implementation Split

**zcoderd (Server-Side)**:
- Accepts `allow_gitignored_files` boolean in workspace payload
- Stores setting in `WorkspaceInfo` struct
- Initializes `GitIgnoreMatcher` when session is created
- Applies filtering to server-side file operations (e.g., `glob` tool)

**zblade (Client-Side)** - **NEEDS IMPLEMENTATION**:
- Provides user setting/toggle for `allow_gitignored_files`
- Sends setting in workspace context when connecting
- Implements `.gitignore` filtering in local file tools:
  - `get_workspace_structure`
  - `read_file` (optional - could warn instead)
  - `list_dir`
  - `grep_search`
  - Any other file traversal operations

## Protocol Changes

### Workspace Payload (zblade → zcoderd)

The workspace object now includes an optional `allow_gitignored_files` field:

```json
{
  "type": "message",
  "session_id": "...",
  "model_id": "...",
  "content": "...",
  "workspace": {
    "root": "/path/to/project",
    "active_file": "src/main.go",
    "allow_gitignored_files": false,  // NEW FIELD (default: false)
    "cursor_position": { ... },
    "open_files": [ ... ]
  }
}
```

### Field Specification

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `allow_gitignored_files` | `boolean` | `false` | When `false`, filter out files matching `.gitignore` patterns. When `true`, include all files. |

## zcoderd Implementation (Completed)

### 1. Data Structures

**`WorkspaceInfo` struct** (`internal/blade/chat.go`):
```go
type WorkspaceInfo struct {
    Root                  string      `json:"root"`
    ProjectID             string      `json:"project_id,omitempty"`
    ActiveFile            string      `json:"active_file"`
    // ... other fields ...
    AllowGitIgnoredFiles  bool        `json:"allow_gitignored_files"` // NEW
}
```

**`Session` struct** (`internal/blade/sessions.go`):
```go
type Session struct {
    // ... other fields ...
    Workspace           *WorkspaceInfo
    GitIgnoreMatcher    *GitIgnoreMatcher // NEW: nil if allow_gitignored_files=true
    // ... other fields ...
}
```

### 2. GitIgnore Matcher

**`GitIgnoreMatcher`** (`internal/blade/gitignore.go`):
- Uses `github.com/sabhiram/go-gitignore` library
- Loads `.gitignore` from workspace root
- Provides `ShouldIgnore(relativePath string) bool` method
- Thread-safe with `sync.RWMutex`

### 3. Session Initialization

When a session is created:
```go
// If allow_gitignored_files is false (default), load .gitignore
var gitignoreMatcher *GitIgnoreMatcher
if workspace != nil && workspace.Root != "" && !workspace.AllowGitIgnoredFiles {
    gitignoreMatcher = NewGitIgnoreMatcher(workspace.Root)
}
```

### 4. Server-Side Tool Filtering

**`glob` tool** (in `websocket_chat.go`):
```go
filepath.WalkDir(session.Workspace.Root, func(path string, d fs.DirEntry, err error) error {
    // ... existing checks ...
    
    // Skip gitignored files if filtering is enabled
    if session.GitIgnoreMatcher != nil && session.GitIgnoreMatcher.ShouldIgnore(relPath) {
        return nil
    }
    
    // ... rest of logic ...
})
```

## zblade Implementation (TODO)

### 1. User Setting

Add a configuration option in zblade settings:

```yaml
# zblade config
workspace:
  allow_gitignored_files: false  # Default: respect .gitignore
```

Or provide a UI toggle in the settings panel.

### 2. Send Setting to zcoderd

When establishing WebSocket connection or sending messages, include the setting:

```typescript
const workspace = {
  root: workspaceRoot,
  active_file: activeFilePath,
  allow_gitignored_files: config.workspace.allow_gitignored_files || false,
  // ... other fields ...
};
```

### 3. Implement Client-Side Filtering

**For `get_workspace_structure`**:
```typescript
// Load .gitignore patterns
const gitignore = loadGitIgnore(workspaceRoot);

function shouldIncludeFile(relativePath: string): boolean {
  if (allowGitIgnoredFiles) {
    return true;
  }
  return !gitignore.ignores(relativePath);
}

// Apply filter when building tree
function buildTree(dir: string, depth: number) {
  // ... traverse directory ...
  if (!shouldIncludeFile(relativePath)) {
    continue; // Skip this file/directory
  }
  // ... add to tree ...
}
```

**For `list_dir`**:
```typescript
function listDirectory(path: string) {
  const entries = fs.readdirSync(path);
  return entries.filter(entry => {
    const relativePath = path.relative(workspaceRoot, path.join(path, entry));
    return shouldIncludeFile(relativePath);
  });
}
```

**For `grep_search`**:
```typescript
// Use ripgrep's built-in gitignore support
const args = ['--json'];
if (!allowGitIgnoredFiles) {
  // ripgrep respects .gitignore by default
  // No additional flags needed
} else {
  args.push('--no-ignore'); // Disable gitignore filtering
}
```

### 4. Recommended Libraries

**Node.js/TypeScript**:
- `ignore` - Fast, spec-compliant `.gitignore` parser
- Built-in with `ripgrep` for grep operations

**Installation**:
```bash
npm install ignore
```

**Usage**:
```typescript
import ignore from 'ignore';
import fs from 'fs';
import path from 'path';

function loadGitIgnore(workspaceRoot: string) {
  const gitignorePath = path.join(workspaceRoot, '.gitignore');
  const ig = ignore();
  
  if (fs.existsSync(gitignorePath)) {
    const gitignoreContent = fs.readFileSync(gitignorePath, 'utf8');
    ig.add(gitignoreContent);
  }
  
  return ig;
}
```

## User Experience

### Default Experience (Secure)
```
User: "Show me the project structure"
AI: [Shows clean tree without node_modules/, .env, etc.]
```

### With Access Enabled
```
User: [Enables "Allow access to gitignored files" in settings]
User: "Show me the project structure"
AI: [Shows complete tree including node_modules/, .env, build/, etc.]
```

### Use Cases for Allowing Gitignored Files
- Debugging build artifacts
- Analyzing dependency issues in `node_modules/`
- Reviewing generated code
- Working with `.env.example` templates

## Security Considerations

1. **Default Deny**: Always default to `allow_gitignored_files: false`
2. **User Consent**: Require explicit user action to enable access
3. **Session Scope**: Setting should be per-session, not global
4. **Visual Indicator**: zblade should show when gitignore filtering is disabled

## Testing

### Test Cases

1. **Default behavior**: Verify `.env`, `node_modules/`, etc. are filtered
2. **Override enabled**: Verify all files are accessible when `allow_gitignored_files: true`
3. **No .gitignore**: Verify graceful handling when `.gitignore` doesn't exist
4. **Invalid .gitignore**: Verify fail-open behavior (allow all files if parsing fails)
5. **Nested .gitignore**: Verify only root `.gitignore` is used (standard Git behavior)

### Example Test Project

```
test-project/
├── .gitignore          # Contains: *.log, .env, node_modules/
├── src/
│   └── main.js        # Should be visible
├── .env               # Should be HIDDEN by default
├── debug.log          # Should be HIDDEN by default
└── node_modules/      # Should be HIDDEN by default
    └── package/
```

## Migration Notes

- **Backward Compatible**: Old zblade clients without this field will default to `false` (secure)
- **No Breaking Changes**: Existing sessions continue to work
- **Gradual Rollout**: Can be deployed to zcoderd first, then zblade

## Related Files

**zcoderd**:
- `internal/blade/chat.go` - WorkspaceInfo struct
- `internal/blade/sessions.go` - Session struct and creation
- `internal/blade/gitignore.go` - GitIgnoreMatcher implementation
- `internal/blade/websocket_chat.go` - Payload parsing and glob tool
- `go.mod` - Added `github.com/sabhiram/go-gitignore` dependency

**zblade** (to be implemented):
- Client configuration
- Workspace payload construction
- File tool implementations (get_workspace_structure, list_dir, etc.)
