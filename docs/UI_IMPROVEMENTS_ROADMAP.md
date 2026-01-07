# Zaguán Blade UI Improvements Roadmap

**Date**: December 30, 2025  
**Status**: In Progress

---

## ✅ Completed

### 1. Enhanced Tool Call Visualization
- **Component**: `ToolCallDisplay.tsx`
- **Features**:
  - Status indicators (pending → executing → complete → error)
  - Animated states with color-coded borders
  - Parsed argument display
  - Tool result display with syntax highlighting
  - Compact, professional design

**Visual Impact**: HIGH - Users will immediately see when tools are executing and their results

---

## 🎯 Next Priority Features

### Tier 1: Immediate Visual Impact (This Week)

#### 1. Progress Indicator Enhancement
**Current State**: Basic progress bar exists  
**Improvements Needed**:
- [ ] Add stage-specific icons (🔍 searching, 📊 analyzing, ✨ synthesizing)
- [ ] Animated stage transitions
- [ ] Estimated time remaining
- [ ] Cancel button for long-running operations

**Files to Modify**:
- `src/components/ChatMessage.tsx` (lines 49-69)

---

#### 2. Tool Result Formatting
**Goal**: Make tool results readable and actionable  
**Features**:
- [ ] Syntax highlighting for code results
- [ ] Collapsible sections for large results
- [ ] Copy button for tool outputs
- [ ] Link detection and rendering

**New Component**: `ToolResultDisplay.tsx`

---

#### 3. Streaming Text Improvements
**Goal**: Better visual feedback during AI response generation  
**Features**:
- [ ] Typing indicator with cursor animation
- [ ] Word-by-word reveal animation
- [ ] Smooth scroll to bottom as content arrives

---

### Tier 2: Standout Features (Next Week)

#### 4. Vertical Diff Blocks (Kiro-inspired) ⭐
**Priority**: VERY HIGH (from THREE_EDITORS_COMPARISON.md)  
**Goal**: Accept/reject individual code changes

**Features**:
- [ ] Side-by-side diff view
- [ ] Accept/reject buttons per hunk
- [ ] Syntax highlighting in diffs
- [ ] Preview before applying
- [ ] Undo/redo support

**New Components**:
- `DiffBlock.tsx` - Individual diff display
- `DiffViewer.tsx` - Container for multiple diffs
- `DiffControls.tsx` - Accept/reject/preview controls

**Backend Integration**:
- [ ] Add diff parsing to `chat_manager.rs`
- [ ] Implement apply_diff command in Tauri
- [ ] Add diff state management

---

#### 5. Quick Edit Mode (Kiro-inspired) ⭐
**Priority**: VERY HIGH  
**Goal**: Fast inline editing without full chat context

**Features**:
- [ ] Keyboard shortcut to trigger (Cmd+K)
- [ ] Inline input overlay
- [ ] Quick suggestions dropdown
- [ ] Apply changes instantly

**New Components**:
- `QuickEditOverlay.tsx`
- `QuickEditInput.tsx`

---

#### 6. Terminal Command Suggestions (Antigravity-inspired)
**Priority**: HIGH  
**Goal**: AI suggests terminal commands with accept/reject

**Features**:
- [ ] Detect when AI suggests commands
- [ ] Show command preview with explanation
- [ ] One-click execute or copy
- [ ] Command history

**New Components**:
- `CommandSuggestion.tsx`
- `CommandPreview.tsx`

---

### Tier 3: Advanced Features (Future)

#### 7. Code Executor (Antigravity-inspired)
**Goal**: Execute AI-generated code safely in sandbox

**Features**:
- [ ] Sandboxed execution environment
- [ ] Output capture and display
- [ ] Error handling and debugging
- [ ] Resource limits

---

#### 8. Checkpoint System (Kiro-inspired)
**Goal**: Save/restore AI edit states

**Features**:
- [ ] Snapshot conversation state
- [ ] Restore previous states
- [ ] Branch from checkpoints
- [ ] Visual timeline

---

#### 9. Codebase Indexing UI
**Goal**: Show what's indexed, force reindex

**Features**:
- [ ] Index status indicator
- [ ] File coverage visualization
- [ ] Manual reindex trigger
- [ ] Index statistics

---

## 🎨 Design Principles

1. **Minimal & Professional**: Clean, dark theme with subtle animations
2. **Information Dense**: Show relevant data without clutter
3. **Responsive Feedback**: Immediate visual response to all actions
4. **Keyboard-First**: All features accessible via keyboard
5. **Status Clarity**: Always show what's happening and why

---

## 📊 Current UI State

### Working Features
- ✅ Basic chat interface
- ✅ Message streaming
- ✅ Tool call display (enhanced)
- ✅ Progress indicators (basic)
- ✅ File explorer
- ✅ Code editor
- ✅ Terminal pane

### Needs Improvement
- ⚠️ Progress indicators (styling)
- ⚠️ Tool results (formatting)
- ⚠️ Diff display (non-existent)
- ⚠️ Quick actions (limited)

---

## 🚀 Implementation Strategy

### Phase 1: Polish Existing (Days 1-2)
1. Enhance progress indicators
2. Improve tool result display
3. Add streaming animations
4. Test with @research command

### Phase 2: Diff System (Days 3-5)
1. Create diff components
2. Implement diff parsing
3. Add accept/reject logic
4. Test with code edits

### Phase 3: Quick Edit (Days 6-7)
1. Create quick edit overlay
2. Add keyboard shortcuts
3. Implement inline suggestions
4. Test workflow

### Phase 4: Advanced Features (Week 2+)
1. Terminal suggestions
2. Code executor
3. Checkpoint system
4. Codebase indexing UI

---

## 🧪 Testing Plan

### Visual Testing
- [ ] Test all tool types (read_file, write_file, grep_search, etc.)
- [ ] Test @research with progress events
- [ ] Test error states
- [ ] Test long-running operations

### Integration Testing
- [ ] zblade ↔ zcoderd communication
- [ ] Tool execution flow
- [ ] Progress event handling
- [ ] Error propagation

### User Experience Testing
- [ ] Keyboard navigation
- [ ] Responsiveness
- [ ] Animation smoothness
- [ ] Information clarity

---

## 📝 Notes

- Focus on **visual impact** first - users need to see progress
- Implement features that make zblade **stand out** from competitors
- Keep the Blade Protocol clean and well-documented
- Test frequently with real workflows
