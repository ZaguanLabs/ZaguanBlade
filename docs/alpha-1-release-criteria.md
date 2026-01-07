# zblade Alpha 1 - Public Release Criteria

## Release Philosophy

**Alpha 1 Goal:** "Can 10 early adopters use zblade productively for 1 week?"

**NOT the goal:** "Daily driver" (that's Beta/1.0)
**NOT the goal:** "Perfect" (that leads to polish paralysis)
**NOT the goal:** "Feature complete" (that's 2.0+)

**The goal:** Useful enough that developers **want** to keep using it despite rough edges.

---

## Release Tiers

### Alpha 1 (Public Alpha)
**Audience:** 10-50 early adopters, tech enthusiasts, AI tool explorers
**Expectation:** Rough edges, bugs, missing features - but **useful**
**Feedback:** Active, direct (Discord, GitHub issues)
**Timeline:** 4-6 weeks from now

### Beta 1 (Private Beta)
**Audience:** 100-500 developers, teams
**Expectation:** Stable enough for side projects, not production
**Feedback:** Structured (surveys, analytics)
**Timeline:** 3-4 months from now

### 1.0 (Public Release)
**Audience:** General developers, daily driver candidates
**Expectation:** Production-ready, stable, documented
**Feedback:** Support tickets, community
**Timeline:** 6-9 months from now

---

## Alpha 1 Release Criteria

### Core Functionality (Must Have)

#### 1. Basic Chat Works
- [x] Send message to AI
- [x] Receive streaming response
- [x] Model selection (Claude, GPT-4, etc.)
- [ ] Message history persists across sessions
- [ ] Clear conversation button
- [ ] Copy message/code blocks

**Success Metric:** User can have a 10-message conversation without crashes

#### 2. File Operations Work
- [x] Open files in editor
- [x] Read file content
- [x] AI can read files via tools
- [ ] AI can write/edit files (apply_patch)
- [ ] Diff view for proposed changes
- [ ] Accept/reject changes
- [ ] Multiple pending edits

**Success Metric:** AI can read a file, suggest changes, and user can apply them

#### 3. Code Editing Works
- [ ] Syntax highlighting (basic)
- [ ] Line numbers
- [ ] Scroll to line (from AI suggestions)
- [ ] Basic search (Ctrl+F)
- [ ] Save file (Ctrl+S)
- [ ] Undo/redo

**Success Metric:** User can edit code comfortably for 30 minutes

#### 4. Project Context Works
- [x] File tree shows project structure
- [ ] AI knows which file is open
- [ ] AI knows cursor position
- [ ] AI can navigate project (list_directory, find_files)
- [ ] Workspace persists across sessions

**Success Metric:** AI can navigate a 50-file project and find relevant code

#### 5. Terminal Integration Works
- [ ] Open terminal pane
- [ ] Run commands (via AI or manually)
- [ ] See command output
- [ ] Multiple terminal tabs
- [ ] Terminal persists across sessions

**Success Metric:** User can run `npm install` and see it complete

---

### Stability (Must Have)

#### 1. No Data Loss
- [ ] Files save reliably
- [ ] Chat history doesn't disappear
- [ ] Pending edits don't vanish
- [ ] Crash recovery (auto-save)

**Success Metric:** 0 reports of lost work in 1 week

#### 2. No Infinite Loops
- [x] Tool loop detection works
- [x] AI doesn't repeat same action endlessly
- [ ] Timeout for long-running operations
- [ ] User can cancel AI operations

**Success Metric:** 0 reports of "AI is stuck" in 1 week

#### 3. Reasonable Performance
- [ ] Chat response starts within 2 seconds
- [ ] File opens within 1 second
- [ ] UI doesn't freeze during AI operations
- [ ] Works with 100+ file projects

**Success Metric:** No complaints about "too slow" in 1 week

#### 4. Error Handling
- [ ] Clear error messages (not stack traces)
- [ ] Graceful degradation (if zcoderd down, show message)
- [ ] Retry logic for network failures
- [ ] User can recover from errors

**Success Metric:** Users understand what went wrong when errors occur

---

### Usefulness (Must Have)

#### 1. Solves Real Problems
- [ ] AI can explain code
- [ ] AI can write simple functions
- [ ] AI can fix bugs (with context)
- [ ] AI can refactor code
- [ ] AI can answer questions about codebase

**Success Metric:** 8/10 users say "this saved me time" after 1 week

#### 2. Better Than Alternatives
- [ ] Faster than copy-pasting to ChatGPT
- [ ] More context-aware than Copilot
- [ ] Easier than Cursor for simple tasks

**Success Metric:** 5/10 users prefer zblade over their current tool for at least one task

#### 3. Onboarding Works
- [ ] First-time user can start chatting in < 2 minutes
- [ ] Clear instructions for setup (zcoderd connection)
- [ ] Example prompts/use cases shown
- [ ] Help documentation exists

**Success Metric:** 9/10 users successfully complete first chat without help

---

### Nice to Have (Not Required for Alpha 1)

#### Deferred to Beta/1.0
- ❌ Multi-language support (use English only for Alpha 1)
- ❌ Local model support (Ollama, etc.)
- ❌ Screenshot capture
- ❌ Visual debugging
- ❌ Multi-level approval
- ❌ Extensions/plugins
- ❌ Team collaboration
- ❌ Git integration (beyond basic commands)
- ❌ Themes/customization
- ❌ Mobile app
- ❌ Web version

#### Why Defer?
These are **differentiators** but not **blockers**. Alpha 1 needs to prove the core value prop: "AI-powered coding assistant that understands your codebase."

Once that works, add the cool stuff.

---

## Release Checklist

### Pre-Release (4 weeks)

**Week 1: Core Functionality**
- [ ] Implement apply_patch tool
- [ ] Add diff view UI
- [ ] Add accept/reject buttons
- [ ] Test with 10 different file types
- [ ] Fix critical bugs

**Week 2: Stability**
- [ ] Add crash recovery (auto-save)
- [ ] Implement timeout for operations
- [ ] Add cancel button for AI operations
- [ ] Test with large projects (1000+ files)
- [ ] Fix memory leaks

**Week 3: Polish**
- [ ] Improve error messages
- [ ] Add onboarding flow
- [ ] Write documentation
- [ ] Create demo video
- [ ] Test on Windows/Mac/Linux

**Week 4: Testing**
- [ ] Internal dogfooding (use zblade to build zblade)
- [ ] Fix top 10 bugs
- [ ] Performance optimization
- [ ] Security review
- [ ] Prepare release notes

### Release Day

**Announcement:**
- [ ] Post on Twitter/X
- [ ] Post on Reddit (r/programming, r/artificial)
- [ ] Post on Hacker News
- [ ] Post on Discord servers
- [ ] Email to waitlist (if any)

**Distribution:**
- [ ] GitHub release with binaries
- [ ] Installation instructions
- [ ] Quick start guide
- [ ] Discord server for support
- [ ] GitHub issues for bug reports

**Monitoring:**
- [ ] Set up error tracking (Sentry?)
- [ ] Set up analytics (usage patterns)
- [ ] Monitor Discord for issues
- [ ] Respond to GitHub issues within 24h

---

## Success Metrics (1 Week Post-Launch)

### Adoption
- **Target:** 50 downloads
- **Stretch:** 100 downloads
- **Measure:** GitHub release downloads

### Retention
- **Target:** 10 users still using after 1 week
- **Stretch:** 20 users still using after 1 week
- **Measure:** Telemetry (opt-in) or Discord activity

### Satisfaction
- **Target:** 7/10 average rating
- **Stretch:** 8/10 average rating
- **Measure:** Post-usage survey

### Feedback
- **Target:** 20 bug reports
- **Stretch:** 50 bug reports
- **Measure:** GitHub issues

### Word of Mouth
- **Target:** 5 organic mentions (Twitter, Reddit, etc.)
- **Stretch:** 10 organic mentions
- **Measure:** Social media monitoring

---

## What Makes Alpha 1 "Good Enough"?

### The Test: Can You Build zblade with zblade?

**Scenario:** Use zblade to:
1. Fix a bug in zblade
2. Add a small feature
3. Refactor a component
4. Write tests

**If you can do this comfortably for 1 hour without major frustration, Alpha 1 is ready.**

### The Test: Can a Stranger Use It?

**Scenario:** Give zblade to someone who's never seen it:
1. Can they install it in < 5 minutes?
2. Can they start chatting in < 2 minutes?
3. Can they complete a simple task (e.g., "explain this function") in < 5 minutes?

**If yes to all three, Alpha 1 is ready.**

### The Test: Would You Tell Your Friends?

**Question:** If a developer friend asked "what are you working on?", would you:
- A) Show them zblade and be excited
- B) Show them zblade but apologize for rough edges
- C) Not show them zblade yet

**If A or B, Alpha 1 is ready. If C, keep working.**

---

## What to Cut (If Running Late)

### Priority 1: Don't Cut
- Chat works
- File reading works
- File editing works (apply_patch)
- Diff view
- Accept/reject changes
- No data loss

### Priority 2: Can Simplify
- Terminal (can be basic, no tabs)
- File tree (can be simple list)
- Editor (can use Monaco defaults)
- Model selection (can default to Claude)

### Priority 3: Can Defer
- Message history persistence (can clear on restart)
- Multiple pending edits (can do one at a time)
- Search (can use browser Ctrl+F)
- Undo/redo (can reload file)

---

## Timeline Estimate

### Optimistic (4 weeks)
- Week 1: Core functionality complete
- Week 2: Stability fixes
- Week 3: Polish
- Week 4: Testing
- **Launch:** End of week 4

### Realistic (6 weeks)
- Week 1-2: Core functionality + bugs
- Week 3-4: Stability + polish
- Week 5: Testing + dogfooding
- Week 6: Final fixes
- **Launch:** End of week 6

### Conservative (8 weeks)
- Week 1-3: Core functionality + major bugs
- Week 4-5: Stability + critical fixes
- Week 6-7: Polish + testing
- Week 8: Final testing + launch prep
- **Launch:** End of week 8

**Recommendation:** Plan for 6 weeks, hope for 4, accept 8.

---

## Post-Alpha 1 Roadmap

### Alpha 2 (2 weeks after Alpha 1)
- Fix top 10 bugs from Alpha 1
- Add most-requested feature
- Improve performance
- Better error messages

### Alpha 3 (4 weeks after Alpha 1)
- Multi-language support (Spanish first)
- Local model support (Ollama)
- Screenshot capture
- Unix socket optimization

### Beta 1 (8 weeks after Alpha 1)
- All Alpha features stable
- Documentation complete
- Onboarding polished
- Ready for 100+ users

### 1.0 (16 weeks after Alpha 1)
- Production-ready
- Daily driver quality
- Full feature set
- Marketing push

---

## Key Insight: Ship Early, Iterate Fast

**The Trap:** "Just one more feature..."
**The Reality:** Early users will tell you what matters

**Better to ship:**
- Alpha 1 with 80% of features working 100%
- Than Alpha 1 with 100% of features working 80%

**Why?**
- Users forgive missing features
- Users don't forgive broken core features
- Feedback guides what to build next

---

## Conclusion

**Alpha 1 Release Criteria:**
1. ✅ Chat works reliably
2. ✅ AI can read and edit files
3. ✅ Diff view shows changes clearly
4. ✅ No data loss
5. ✅ No infinite loops
6. ✅ Useful for at least one real task
7. ✅ 10 people can use it for 1 week

**Not Required:**
- ❌ Perfect
- ❌ Feature complete
- ❌ Daily driver ready
- ❌ Polished UI

**Timeline:** 4-6 weeks (realistic: 6)

**Success:** 10 users still using after 1 week, 7/10 satisfaction

**Next Step:** Focus on core functionality (apply_patch, diff view, stability) for the next 2 weeks, then reassess.

---

**Remember:** Done is better than perfect. Ship Alpha 1, get feedback, iterate. That's how you win.
