# zblade Architecture & Best Practices

This document outlines architectural patterns, best practices, and future-proofing strategies for zblade development.

---

## Table of Contents

1. [Rust Backend Patterns](#rust-backend-patterns)
2. [Tauri Integration](#tauri-integration)
3. [React/Next.js Frontend](#reactnextjs-frontend)
4. [State Management](#state-management)
5. [Error Handling](#error-handling)
6. [Performance](#performance)
7. [Security](#security)
8. [Testing](#testing)
9. [Future-Proofing](#future-proofing)

---

## Rust Backend Patterns

### 1. **Use `Result<T, E>` for All Fallible Operations**

**Why:** Rust's type system forces error handling. Never use `.unwrap()` or `.expect()` in production code.

```rust
// ❌ Bad - Will panic on error
let content = fs::read_to_string(path).unwrap();

// ✅ Good - Explicit error handling
let content = fs::read_to_string(path)
    .map_err(|e| format!("Failed to read file: {}", e))?;
```

**Current Status:** zblade uses this pattern in `approve_edit` and other commands.

---

### 2. **Use `Arc<Mutex<T>>` for Shared State**

**Why:** Rust's ownership system requires explicit synchronization for shared mutable state.

```rust
// AppState uses Arc<Mutex<>> for thread-safe state sharing
pub struct AppState {
    pub pending_edits: Mutex<HashMap<String, PendingEdit>>,
    pub workspace: Mutex<WorkspaceManager>,
}
```

**Best Practice:**
- Keep locks short-lived (acquire, modify, release quickly)
- Never hold multiple locks simultaneously (deadlock risk)
- Use `drop(lock)` to release early if needed

---

### 3. **Separate Business Logic from Tauri Commands**

**Pattern:**
```rust
// ❌ Bad - Logic mixed with command
#[tauri::command]
async fn do_something(state: State<'_, AppState>) -> Result<(), String> {
    // 100 lines of business logic here
}

// ✅ Good - Logic in separate module
#[tauri::command]
async fn do_something(state: State<'_, AppState>) -> Result<(), String> {
    business_logic::do_something(&state)
}
```

**Why:** Testability, reusability, and separation of concerns.

---

### 4. **Use Serde for All Data Transfer**

**Why:** Type-safe serialization between Rust and TypeScript.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyPayload {
    pub field: String,
}
```

**Best Practice:**
- Always derive `Serialize` and `Deserialize` for payloads
- Use `#[serde(rename_all = "camelCase")]` for JavaScript compatibility
- Use `Option<T>` for optional fields

---

### 5. **Module Organization**

**Current Structure:**
```
src-tauri/src/
├── lib.rs              # Tauri commands, app state
├── events.rs           # Event definitions (NEW)
├── chat_manager.rs     # Chat logic
├── tools.rs            # Tool execution
├── ai_workflow.rs      # AI workflow orchestration
├── workspace_manager.rs # Workspace state
└── ...
```

**Best Practice:**
- One module per major feature
- Keep `lib.rs` thin (just command registration)
- Use `pub mod` to expose modules
- Use `pub(crate)` for internal-only items

---

## Tauri Integration

### 1. **Command Naming Convention**

**Pattern:** `verb_noun` (snake_case)

```rust
// ✅ Good
approve_edit
reject_edit
list_files
read_file_content
approve_edits_for_file

// ❌ Bad
approveEdit  // camelCase
EditApprove  // PascalCase
approve      // Too vague
```

---

### 2. **Event Naming Convention**

**Pattern:** `noun-verb` or `noun-state` (kebab-case)

```rust
// ✅ Good
"file-saved"
"edit-applied"
"connection-status"
"chat-update"

// ❌ Bad
"fileSaved"     // camelCase
"file_saved"    // snake_case
"saved-file"    // Wrong order
```

**Why:** Consistency with web standards and JavaScript conventions.

---

### 3. **Emit Events, Don't Return Large Data**

**Pattern:**
```rust
// ❌ Bad - Blocking command that returns large data
#[tauri::command]
async fn search_workspace(query: String) -> Result<Vec<SearchResult>, String> {
    // Blocks until all results found
    Ok(results)
}

// ✅ Good - Async with progress events
#[tauri::command]
async fn search_workspace(query: String, app: AppHandle) -> Result<(), String> {
    tokio::spawn(async move {
        for result in search_iter {
            app.emit("search-progress", result)?;
        }
        app.emit("search-complete", ())?;
    });
    Ok(())
}
```

**Why:** Responsive UI, progress feedback, cancellation support.

---

### 4. **Use `AppHandle` for Background Tasks**

**Pattern:**
```rust
#[tauri::command]
async fn long_operation(app: AppHandle) -> Result<(), String> {
    tokio::spawn(async move {
        // Long-running work
        app.emit("progress", payload)?;
    });
    Ok(()) // Returns immediately
}
```

**Why:** Don't block the UI thread. Tauri commands should return quickly.

---

### 5. **State Management in Tauri**

**Best Practice:**
```rust
// ✅ Use Tauri's managed state
pub struct AppState {
    // Shared state here
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::new())  // Single instance
        .invoke_handler(...)
        .run(...)
}

// Access in commands
#[tauri::command]
fn my_command(state: State<'_, AppState>) {
    // Use state
}
```

**Why:** Thread-safe, single source of truth, automatic lifetime management.

---

## React/Next.js Frontend

### 1. **Use TypeScript Everywhere**

**Why:** Type safety prevents runtime errors, especially with Tauri's `invoke()` and events.

```typescript
// ✅ Good - Typed
import { invoke } from '@tauri-apps/api/core';
import { EditAppliedPayload } from '@/types/events';

const result = await invoke<string>('approve_edit', { editId });

// ❌ Bad - Untyped
const result = await invoke('approve_edit', { editId });
```

---

### 2. **Custom Hooks for Tauri Integration**

**Pattern:**
```typescript
// ✅ Good - Reusable hook
function useEditWorkflow() {
  const [pendingEdits, setPendingEdits] = useState<EditProposal[]>([]);
  
  useEffect(() => {
    const unlisten = listen<ProposeEditPayload>('propose-edit', (event) => {
      setPendingEdits(prev => [...prev, event.payload]);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);
  
  const approveEdit = async (id: string) => {
    await invoke('approve_edit', { editId: id });
    setPendingEdits(prev => prev.filter(e => e.id !== id));
  };
  
  return { pendingEdits, approveEdit };
}

// ❌ Bad - Logic in component
function MyComponent() {
  // 100 lines of Tauri logic here
}
```

**Current Example:** `useChat` hook in zblade.

---

### 3. **Event Listener Cleanup**

**Critical Pattern:**
```typescript
useEffect(() => {
  let unlisten: (() => void) | undefined;
  
  const setup = async () => {
    unlisten = await listen('my-event', handler);
  };
  
  setup();
  
  return () => {
    if (unlisten) unlisten();  // MUST cleanup
  };
}, [dependencies]);
```

**Why:** Memory leaks. Listeners accumulate on every re-render without cleanup.

---

### 4. **Separate Presentation from Logic**

**Pattern:**
```typescript
// ✅ Good - Logic in hook, presentation in component
function useFileOperations() {
  // All Tauri logic here
  return { files, openFile, saveFile };
}

function FileExplorer() {
  const { files, openFile } = useFileOperations();
  return <div>{/* Pure UI */}</div>;
}

// ❌ Bad - Mixed concerns
function FileExplorer() {
  const [files, setFiles] = useState([]);
  useEffect(() => {
    invoke('list_files').then(setFiles);
  }, []);
  return <div>{/* UI + logic mixed */}</div>;
}
```

---

### 5. **Optimize Re-renders**

**Pattern:**
```typescript
// ✅ Good - Memoized callbacks
const approveEdit = useCallback(async (id: string) => {
  await invoke('approve_edit', { editId: id });
}, []);

// ✅ Good - Memoized expensive computations
const filteredEdits = useMemo(() => 
  edits.filter(e => e.status === 'pending'),
  [edits]
);

// ❌ Bad - New function on every render
function MyComponent() {
  const approveEdit = async (id: string) => {
    // This creates a new function every render
  };
}
```

**Why:** Performance. Prevents unnecessary re-renders of child components.

---

## State Management

### 1. **Single Source of Truth**

**Architecture:**
```
Rust Backend (AppState)
    ↓ (events)
React Hooks (useState)
    ↓ (props)
React Components
```

**Rule:** Backend owns the truth. Frontend is a view.

---

### 2. **State Synchronization Pattern**

```typescript
// Backend emits event
app.emit("edit-applied", { edit_id, file_path })?;

// Frontend listens and updates local state
useEffect(() => {
  const unlisten = listen<EditAppliedPayload>('edit-applied', (event) => {
    setPendingEdits(prev => prev.filter(e => e.id !== event.payload.edit_id));
  });
  return () => unlisten.then(fn => fn());
}, []);
```

**Why:** Backend and frontend stay in sync automatically.

---

### 3. **Avoid Prop Drilling**

**Pattern:**
```typescript
// ✅ Good - Context for global state
const EditContext = createContext<EditContextType>(null!);

function App() {
  const editState = useEditWorkflow();
  return (
    <EditContext.Provider value={editState}>
      <DeepComponent />
    </EditContext.Provider>
  );
}

function DeepComponent() {
  const { approveEdit } = useContext(EditContext);
  // No prop drilling needed
}

// ❌ Bad - Props through 5 levels
<A edits={edits}>
  <B edits={edits}>
    <C edits={edits}>
      <D edits={edits}>
        <E edits={edits} />
```

---

## Error Handling

### 1. **Rust Error Propagation**

**Pattern:**
```rust
// ✅ Good - Use ? operator
fn my_function() -> Result<String, String> {
    let content = fs::read_to_string(path)?;  // Propagates error
    let parsed = parse_content(&content)?;
    Ok(parsed)
}

// ❌ Bad - Swallowing errors
fn my_function() -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(_) => Ok(String::new()),  // Error lost!
    }
}
```

---

### 2. **Frontend Error Handling**

**Pattern:**
```typescript
// ✅ Good - Explicit error handling
try {
  await invoke('approve_edit', { editId });
  showSuccess('Edit applied');
} catch (error) {
  console.error('Failed to apply edit:', error);
  showError(`Failed: ${error}`);
}

// ❌ Bad - Silent failure
invoke('approve_edit', { editId });  // What if it fails?
```

---

### 3. **Error Event Pattern**

**Best Practice:**
```rust
// Backend emits error events
if let Err(e) = operation() {
    app.emit("backend-error", BackendErrorPayload {
        error: e.to_string(),
        context: Some("approve_edit".to_string()),
    })?;
}

// Frontend listens globally
useEffect(() => {
  const unlisten = listen<BackendErrorPayload>('backend-error', (event) => {
    showErrorNotification(event.payload.error);
  });
  return () => unlisten.then(fn => fn());
}, []);
```

**Why:** Centralized error handling, consistent UX.

---

## Performance

### 1. **Rust: Use Async for I/O**

**Pattern:**
```rust
// ✅ Good - Non-blocking I/O
#[tauri::command]
async fn read_large_file(path: String) -> Result<String, String> {
    tokio::fs::read_to_string(path)
        .await
        .map_err(|e| e.to_string())
}

// ❌ Bad - Blocks thread
#[tauri::command]
fn read_large_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
}
```

---

### 2. **Rust: Avoid Cloning Large Data**

**Pattern:**
```rust
// ✅ Good - Pass by reference
fn process_data(data: &[u8]) {
    // Work with reference
}

// ❌ Bad - Unnecessary clone
fn process_data(data: Vec<u8>) {
    // Copies entire vector
}
```

---

### 3. **React: Virtualize Large Lists**

**Pattern:**
```typescript
// ✅ Good - Only render visible items
import { FixedSizeList } from 'react-window';

<FixedSizeList
  height={600}
  itemCount={10000}
  itemSize={35}
>
  {Row}
</FixedSizeList>

// ❌ Bad - Render all 10,000 items
{items.map(item => <Row key={item.id} {...item} />)}
```

---

### 4. **Debounce Expensive Operations**

**Pattern:**
```typescript
// ✅ Good - Debounced search
const debouncedSearch = useMemo(
  () => debounce((query: string) => {
    invoke('search_workspace', { query });
  }, 300),
  []
);

// ❌ Bad - Search on every keystroke
onChange={(e) => invoke('search_workspace', { query: e.target.value })}
```

---

## Security

### 1. **Validate All Input in Rust**

**Pattern:**
```rust
#[tauri::command]
fn read_file(path: String) -> Result<String, String> {
    // ✅ Validate path is within workspace
    let workspace = get_workspace_root();
    let full_path = workspace.join(&path);
    
    if !full_path.starts_with(&workspace) {
        return Err("Path outside workspace".to_string());
    }
    
    fs::read_to_string(full_path)
        .map_err(|e| e.to_string())
}
```

**Why:** Never trust frontend input. Always validate in backend.

---

### 2. **Use Tauri's Allowlist**

**Pattern in `tauri.conf.json`:**
```json
{
  "tauri": {
    "allowlist": {
      "fs": {
        "scope": ["$APPDATA/**", "$RESOURCE/**"]
      },
      "shell": {
        "open": true,
        "scope": []
      }
    }
  }
}
```

**Why:** Principle of least privilege. Only allow what's needed.

---

### 3. **Sanitize File Paths**

**Pattern:**
```rust
use std::path::Path;

fn sanitize_path(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    
    // Reject paths with ..
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Invalid path".to_string());
    }
    
    Ok(path.to_path_buf())
}
```

---

## Testing

### 1. **Rust Unit Tests**

**Pattern:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sanitize_path() {
        assert!(sanitize_path("../etc/passwd").is_err());
        assert!(sanitize_path("src/main.rs").is_ok());
    }
}
```

**Run:** `cargo test`

---

### 2. **React Component Tests**

**Pattern:**
```typescript
import { render, screen } from '@testing-library/react';

test('renders edit overlay', () => {
  render(<EditorDiffOverlay {...props} />);
  expect(screen.getByText('Accept')).toBeInTheDocument();
});
```

---

### 3. **Integration Tests**

**Pattern:**
```rust
#[tokio::test]
async fn test_approve_edit_flow() {
    let state = AppState::new();
    
    // Add pending edit
    state.pending_edits.lock().unwrap().insert(
        "test-id".to_string(),
        create_test_edit(),
    );
    
    // Approve it
    let result = approve_edit("test-id".to_string(), &state).await;
    
    assert!(result.is_ok());
    assert!(state.pending_edits.lock().unwrap().is_empty());
}
```

---

## Future-Proofing

### 1. **Version Your Events**

**Pattern:**
```rust
// When breaking changes needed
pub const EDIT_APPLIED_V2: &str = "edit-applied-v2";

#[derive(Serialize)]
pub struct EditAppliedPayloadV2 {
    pub edit_id: String,
    pub file_path: String,
    pub timestamp: u64,  // New field
}
```

**Why:** Allows gradual migration without breaking existing code.

---

### 2. **Use Feature Flags**

**Pattern:**
```rust
#[cfg(feature = "experimental")]
pub fn experimental_feature() {
    // New feature code
}
```

**In `Cargo.toml`:**
```toml
[features]
default = []
experimental = []
```

---

### 3. **Document Breaking Changes**

**Pattern:** Keep a `CHANGELOG.md`
```markdown
## [Unreleased]
### Breaking Changes
- Renamed `file-changed` to `file-modified`
- Changed `EditPayload.path` from `String` to `PathBuf`

### Migration Guide
- Update event listeners from `file-changed` to `file-modified`
- Convert paths: `PathBuf::from(payload.path)`
```

---

### 4. **Use Semantic Versioning**

**Pattern:**
- `1.0.0` → `1.0.1`: Bug fixes (patch)
- `1.0.0` → `1.1.0`: New features (minor)
- `1.0.0` → `2.0.0`: Breaking changes (major)

---

### 5. **Deprecation Strategy**

**Pattern:**
```rust
#[deprecated(since = "1.2.0", note = "Use `approve_edits_for_file` instead")]
#[tauri::command]
fn old_approve_edit() {
    // Keep for backward compatibility
}
```

**Why:** Gives users time to migrate before removal.

---

## Summary: Key Principles

1. **Type Safety First** - Use Rust's and TypeScript's type systems fully
2. **Events Over Polling** - Reactive architecture, not request/response
3. **Backend Owns Truth** - Frontend is a view of backend state
4. **Fail Explicitly** - Never swallow errors
5. **Test Early** - Unit tests prevent regressions
6. **Document Everything** - Code changes, events change, architecture changes
7. **Version Carefully** - Semantic versioning, deprecation notices
8. **Optimize Later** - Correctness first, performance second
9. **Secure by Default** - Validate all input, minimize privileges
10. **Think Long-term** - Design for the IDE you want in 5 years

---

## Resources

- **Rust Book**: https://doc.rust-lang.org/book/
- **Tauri Docs**: https://tauri.app/
- **React Docs**: https://react.dev/
- **Next.js Docs**: https://nextjs.org/docs

---

## Getting Help

When stuck:
1. Check event contract (`EVENTS.md`, `FUTURE_EVENTS.md`)
2. Look at existing patterns in codebase
3. Read this architecture guide
4. Ask in team chat with specific code examples
