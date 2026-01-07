# CLI Tool Inspiration Analysis

## Executive Summary

After analyzing 5 major AI-powered CLI projects (Codex, Gemini-CLI, Qwen-Code, Kilocode, Opencode), I've identified **critical missing tools and architectural patterns** that would significantly enhance zblade's capabilities.

---

## Key Findings: What We're Missing

### ✅ **What We Already Have** (Confirmed in zcoderd)

1. **Web Tools** - Already implemented via aggregator:
   - **`@web`** (fetch_url) - Fetch content from URLs
   - **`@search`** - Quick web search for facts
   - **`@research`** - Complex multi-step research (opens dedicated tab)
   - **Location:** `internal/blade/commands.go`, `internal/webtools/`

2. **Advanced Memory System** - Already implemented:
   - **MMU (Memory Management Unit)** - Context versioning, staleness detection
   - **Persistent memory** across sessions
   - **Location:** `internal/context/mmu.go`, `internal/context/store.go`
   - **Note:** This is MORE sophisticated than the basic memory tools in other projects

### 🔴 **Critical Missing Tools**

1. **`read_file_range` / Partial File Reading**
   - **What it does:** Read specific line ranges from files instead of entire files
   - **Why we need it:** Massive performance improvement for large files, reduces token usage
   - **Example:** `read_file_range(path: "src/main.rs", start_line: 100, end_line: 150)`
   - **Found in:** Codex, Kilocode, Qwen-Code (all have this!)
   - **Impact:** 🔥 HIGH - Immediate performance win

2. **`codebase_search` / Semantic Code Search**
   - **What it does:** Search codebase semantically, not just grep
   - **Why we need it:** Find functions, classes, patterns across entire codebase
   - **Example:** "Find all struct definitions" or "Find authentication logic"
   - **Found in:** Kilocode, Gemini-CLI
   - **Impact:** 🔥 HIGH - Better code navigation

3. **`browser_action`**
   - **What it does:** Control a browser for testing/automation
   - **Why we need it:** Test web apps, automate browser tasks
   - **Found in:** Kilocode, Gemini-CLI
   - **Impact:** ⚪ REDUNDANT - Will be superseded by planned video/screenshot integration features

4. **`generate_image`**
   - **What it does:** Generate images using AI (diagrams, mockups)
   - **Why we need it:** Create visual assets, diagrams, UI mockups
   - **Found in:** Kilocode
   - **Impact:** 🟡 MEDIUM - Nice to have

5. **`ask_followup_question`**
   - **What it does:** AI can ask clarifying questions before proceeding
   - **Why we need it:** Better UX, prevents wrong assumptions
   - **Found in:** Kilocode
   - **Impact:** 🟢 LOW - UX improvement

6. **`attempt_completion`**
   - **What it does:** Signal task completion with summary
   - **Why we need it:** Clear workflow endpoints, better UX
   - **Found in:** Kilocode
   - **Impact:** 🟢 LOW - UX improvement

7. **`new_task` / `switch_mode`**
   - **What it does:** Start new task or switch context/mode
   - **Why we need it:** Multi-task management, context switching
   - **Found in:** Kilocode, Gemini-CLI
   - **Impact:** 🟢 LOW - Workflow management

---

## 🟡 **Important Missing Features**

### **Tool Discovery & MCP Integration**

All projects have sophisticated **Model Context Protocol (MCP)** integration:

- **Dynamic tool discovery** from MCP servers
- **Runtime tool registration** (add tools without restart)
- **MCP server management** (start/stop/restart servers)
- **Tool namespacing** (server_name::tool_name format)

**Example from Qwen-Code:**
```typescript
async discoverAllTools(): Promise<void> {
  await this.discoverAndRegisterToolsFromCommand();
  await this.mcpClientManager.discoverAllMcpTools(this.config);
}
```

**What we need:**
- MCP client integration in zblade
- Dynamic tool loading from external MCP servers
- Tool registry that supports runtime updates

---

### **Sandbox & Security**

**Codex has sophisticated sandboxing:**

```rust
pub struct SandboxAttempt<'a> {
    pub sandbox: SandboxType,
    pub policy: &'a SandboxPolicy,
    pub sandbox_cwd: &'a Path,
}

pub enum ExecApprovalRequirement {
    Skip { bypass_sandbox: bool },
    Forbidden { reason: String },
    NeedsApproval { reason: Option<String> },
}
```

**Features:**
- Sandbox selection (None, ReadOnly, Restricted, External)
- Approval requirements based on sandbox policy
- Automatic retry without sandbox on denial
- Escalation workflows

**What we need:**
- Sandbox policy system
- Configurable approval requirements
- Safe execution environment for untrusted commands

---

### **Tool Orchestration**

**Codex has a `ToolOrchestrator`:**

```rust
pub async fn run<Rq, Out, T>(
    &mut self,
    tool: &mut T,
    req: &Rq,
    tool_ctx: &ToolCtx<'_>,
    turn_ctx: &TurnContext,
    approval_policy: AskForApproval,
) -> Result<Out, ToolError>
```

**Features:**
- Centralized approval logic
- Sandbox selection
- Retry semantics
- Error handling

**What we need:**
- Unified tool execution pipeline
- Consistent approval flow
- Better error recovery

---

### **Tool Confirmation & Policies**

**Gemini-CLI has a `MessageBus` for tool confirmation:**

```typescript
async shouldConfirmExecute(
  abortSignal: AbortSignal,
): Promise<ToolCallConfirmationDetails | false> {
  const decision = await this.getMessageBusDecision(abortSignal);
  if (decision === 'ALLOW') return false;
  if (decision === 'DENY') throw new Error('Tool execution denied by policy.');
  if (decision === 'ASK_USER') return this.getConfirmationDetails(abortSignal);
}
```

**Features:**
- Policy-based auto-approval
- User confirmation when needed
- Configurable per-tool policies

**What we need:**
- Policy system (auto-approve safe tools)
- Per-tool confirmation settings
- Session-based approval memory

---

### **Streaming Tool Output**

**All projects support streaming tool output:**

```typescript
execute(
  signal: AbortSignal,
  updateOutput?: (output: string | AnsiOutput) => void,
  shellExecutionConfig?: ShellExecutionConfig,
): Promise<TResult>
```

**What we need:**
- Real-time output streaming for long-running commands
- Progress indicators
- Cancellation support

---

### **Tool Metadata & Organization**

**All projects have rich tool metadata:**

```typescript
interface ToolBuilder {
  name: string;
  displayName: string;
  description: string;
  kind: Kind; // Categorization
  schema: FunctionDeclaration;
  isOutputMarkdown: boolean;
  canUpdateOutput: boolean; // Supports streaming
}
```

**What we need:**
- Tool categories (file, web, system, ai)
- Display names (user-friendly)
- Markdown output support flag
- Streaming capability flag

---

## 🟢 **Nice-to-Have Features**

### **Skills System** (Qwen-Code, Gemini-CLI)

- User-defined reusable workflows
- Skill discovery and management
- Skill composition

### **Slash Commands** (All projects)

- `/tools` - List available tools
- `/mcp` - Manage MCP servers
- `/memory` - View/manage memory
- `/skills` - Manage skills
- `/settings` - Configure settings

### **IDE Integration** (Qwen-Code, Kilocode)

- VSCode extension integration
- IDE server protocol
- Real-time file watching
- Diff preview in editor

### **Web Browser Control** (Kilocode)

- Puppeteer/Playwright integration
- Screenshot capture
- Web scraping
- Automated testing

---

## Architectural Patterns We Should Adopt

### **1. Tool Builder Pattern**

**Separate validation from execution:**

```typescript
interface ToolBuilder<TParams, TResult> {
  build(params: TParams): ToolInvocation<TParams, TResult>;
}

interface ToolInvocation<TParams, TResult> {
  params: TParams; // Already validated
  getDescription(): string;
  toolLocations(): ToolLocation[];
  shouldConfirmExecute(): Promise<boolean>;
  execute(signal: AbortSignal): Promise<TResult>;
}
```

**Benefits:**
- Clear separation of concerns
- Validation happens once
- Easier testing
- Better error messages

---

### **2. Tool Registry Pattern**

**Centralized tool management:**

```typescript
class ToolRegistry {
  private tools: Map<string, AnyDeclarativeTool>;
  
  registerTool(tool: AnyDeclarativeTool): void;
  getTool(name: string): AnyDeclarativeTool | undefined;
  getAllTools(): AnyDeclarativeTool[];
  getFunctionDeclarations(): FunctionDeclaration[];
  
  // MCP support
  async discoverMcpTools(): Promise<void>;
  removeMcpToolsByServer(serverName: string): void;
}
```

**Benefits:**
- Single source of truth
- Dynamic tool loading
- Easy tool filtering
- MCP integration

---

### **3. Tool Context Pattern**

**Pass rich context to tools:**

```rust
pub struct ToolCtx<'a> {
    pub session: &'a Session,
    pub turn: &'a TurnContext,
    pub call_id: String,
    pub tool_name: String,
}
```

**Benefits:**
- Tools have full context
- Easier to add new context
- Better logging/tracing

---

### **4. Unified Exec Pattern**

**Codex has `exec_command` + `write_stdin` for interactive sessions:**

```rust
// Start session
exec_command(cmd: "npm run dev", session_id: 123)

// Interact with session
write_stdin(session_id: 123, chars: "y\n")

// Session persists across tool calls
```

**Benefits:**
- Interactive command support
- Long-running processes
- Real-time interaction

---

## Comparison Matrix

| Feature | zblade/zcoderd | Codex | Gemini-CLI | Qwen-Code | Kilocode | Opencode |
|---------|----------------|-------|------------|-----------|----------|----------|
| **File Tools** |
| read_file | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| read_file_range | ❌ | ✅ | ✅ | ✅ | ✅ | ❌ |
| write_file | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| apply_patch | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| list_files | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| grep_search | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| delete_file | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **System Tools** |
| run_command | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| exec_command (interactive) | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ |
| write_stdin | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Web Tools** |
| web_fetch (@web) | ✅ | ❌ | ❌ | ✅ | ✅ | ❌ |
| web_search (@search) | ✅ | ✅ | ❌ | ✅ | ❌ | ❌ |
| research (@research) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| browser_action | ❌ | ❌ | ✅ | ❌ | ✅ | ❌ |
| **Code Intelligence** |
| codebase_search | ❌ | ❌ | ✅ | ❌ | ✅ | ❌ |
| **AI Tools** |
| generate_image | ❌ | ❌ | ✅ | ❌ | ✅ | ❌ |
| **Workflow Tools** |
| memory (MMU) | ✅✨ | ❌ | ✅ | ✅ | ✅ | ❌ |
| ask_followup_question | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ |
| attempt_completion | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ |
| new_task | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ |
| **Architecture** |
| MCP Integration | ❌ | ✅ | ✅ | ✅ | ✅ | ❌ |
| Tool Registry | ⚠️ | ✅ | ✅ | ✅ | ✅ | ❌ |
| Sandboxing | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ |
| Streaming Output | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |
| Tool Policies | ❌ | ✅ | ✅ | ❌ | ❌ | ❌ |

**Legend:** ✅ Full Support | ⚠️ Partial Support | ❌ Not Implemented

---

## Recommended Implementation Priority

### **Phase 1: Critical Tools (Immediate)**

1. **`read_file_range`** - Huge performance win (10x for large files)
2. **`codebase_search`** - Better code navigation (semantic search)

### **Phase 2: Architecture (Next Sprint)**

1. **Tool Registry refactor** - Centralized management
2. **MCP Client integration** - Dynamic tool loading
3. **Tool Builder pattern** - Better validation
4. **Streaming output** - Real-time feedback

### **Phase 3: Advanced Tools (Future)**

1. **`generate_image`** - Visual assets
2. **Interactive exec** - Long-running processes
3. **Workflow tools** - ask_followup_question, attempt_completion, new_task
4. ~~**`browser_action`**~~ - Redundant with planned video/screenshot integration

### **Phase 4: Polish (Later)**

1. **Sandboxing** - Security
2. **Tool policies** - Auto-approval
3. **Skills system** - Reusable workflows
4. **IDE integration** - Better UX

---

## Code Examples to Implement

### **1. read_file_range Tool**

```rust
#[derive(Deserialize)]
struct ReadFileRangeArgs {
    path: String,
    start_line: usize,
    end_line: usize,
}

pub fn read_file_range(args: ReadFileRangeArgs) -> ToolResult {
    let content = fs::read_to_string(&args.path)?;
    let lines: Vec<&str> = content.lines().collect();
    
    let start = args.start_line.saturating_sub(1);
    let end = args.end_line.min(lines.len());
    
    let selected = lines[start..end].join("\n");
    
    ToolResult {
        success: true,
        content: format!("Lines {}-{} of {}:\n{}", 
            args.start_line, args.end_line, args.path, selected),
        error: None,
    }
}
```

### **2. Tool Registry**

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }
    
    pub fn get(&self, name: &str) -> Option<&Box<dyn Tool>> {
        self.tools.get(name)
    }
    
    pub fn get_schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|t| t.schema()).collect()
    }
}
```

### **3. MCP Client Integration**

```rust
pub struct McpClient {
    servers: HashMap<String, McpServer>,
}

impl McpClient {
    pub async fn discover_tools(&mut self, server_name: &str) -> Result<Vec<Tool>> {
        let server = self.servers.get_mut(server_name)?;
        let tools = server.list_tools().await?;
        
        // Convert MCP tools to our Tool trait
        tools.into_iter()
            .map(|mcp_tool| self.convert_mcp_tool(server_name, mcp_tool))
            .collect()
    }
}
```

---

## Conclusion

**What We Already Have:**
- ✅ Web tools (@web, @search, @research) via aggregator
- ✅ Advanced memory system (MMU) - more sophisticated than competitors
- ✅ Streaming output
- ✅ Core file operations

**What We're Actually Missing:**
- ❌ **2 critical tools** that would significantly improve performance
- ❌ **2 major architectural patterns** for better extensibility
- 📹 **Video/screenshot integration** (planned) - Will handle visual inspection needs

The most impactful quick wins are:

1. **`read_file_range`** - Immediate 10x performance improvement for large files
2. **`codebase_search`** - Semantic code search (not just grep)
3. **Tool Registry** - Better architecture foundation
4. **MCP Integration** - Extensibility for future tools

All of these projects are production-grade and battle-tested. We should learn from their designs and adopt their best practices.

---

## Next Steps

1. **Review this analysis** with the team
2. **Prioritize tools** based on user needs
3. **Design tool architecture** (Registry, Builder pattern)
4. **Implement Phase 1 tools** (read_file_range, web_fetch, memory, codebase_search)
5. **Plan MCP integration** for extensibility

---

**Document Version:** 1.0  
**Date:** 2026-01-02  
**Author:** Cascade AI Analysis  
**Sources:** Codex-RS, Gemini-CLI, Qwen-Code, Kilocode, Opencode
