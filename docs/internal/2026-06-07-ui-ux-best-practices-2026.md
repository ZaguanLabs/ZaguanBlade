# UI/UX Best Practices 2026 Research and Zaguan Blade UI Plan

Date: 2026-06-07

## Executive summary

The suspicion is mostly correct: the current UI has a real desktop-app shell, but its visual language often reads like a web product page or SaaS dashboard. React, TypeScript, Vite, and Tauri are not the cause. The cause is the design system we have layered on top: floating cards, rounded panels, heavy shadows, glass/elevation vocabulary, marketing-style empty states, accent-tinted pills, and custom widgets that do not always follow desktop semantics.

The right target is not "native-looking at all costs." The right target is a productivity desktop application built with web technology: dense, predictable, keyboard-first, pane-based, fast under continuous interaction, accessible through standard GUI patterns, and restrained visually so the editor, terminal, file tree, git state, and AI workflow carry the interface.

Recommended direction:

1. Replace the "floating card workspace" model with an integrated pane/chrome model.
2. Create a desktop design system with tokens for panes, toolbars, rows, fields, popovers, dialogs, status bars, and selection states.
3. Make keyboard and command discoverability a first-class product surface, not an afterthought.
4. Bring custom React widgets in line with WAI-ARIA desktop GUI conventions.
5. Instrument interaction performance and treat 200 ms INP-equivalent responsiveness as a hard UX budget.
6. Reframe AI UI around transparency, review, control, and recoverability instead of decorative "AI" styling.

## Research basis

Sources reviewed online on 2026-06-07:

- [Microsoft Fluent 2 Layout](https://fluent2.microsoft.design/layout): spacing ramps, grid, baseline grid, and responsive layout as system foundations.
- [Microsoft Fluent 2 Accessibility](https://fluent2.microsoft.design/accessibility): accessibility as a foundation, with WCAG AA as the minimum.
- [Microsoft Fluent 2 Material](https://fluent2.microsoft.design/material): solid surfaces as the default; acrylic/glass-like material for transient surfaces only.
- [Apple Human Interface Guidelines: The menu bar](https://developer.apple.com/design/human-interface-guidelines/the-menu-bar): Mac users rely on the menu bar to learn commands and discover shortcuts.
- [Apple Human Interface Guidelines: Toolbars](https://developer.apple.com/design/human-interface-guidelines/toolbars): macOS toolbar guidance emphasizes predictable access to essential view actions.
- [Apple Human Interface Guidelines: Windows](https://developer.apple.com/design/human-interface-guidelines/windows): windows define app boundaries and include system-provided controls and frame behavior.
- [GNOME HIG: Standard keyboard shortcuts](https://developer.gnome.org/hig/reference/keyboard.html): common desktop shortcuts such as Ctrl+W, Ctrl+Q, F9, F10, Ctrl+?, and Ctrl+,.
- [WCAG 2.2 changes](https://www.w3.org/WAI/standards-guidelines/wcag/new-in-22/): focus not obscured, dragging alternatives, and minimum target size.
- [WAI-ARIA Authoring Practices Guide](https://wai-aria-practices.netlify.app/aria-practices/): rich web widgets should borrow desktop GUI keyboard conventions and remain keyboard-operable.
- [web.dev: Optimize Interaction to Next Paint](https://web.dev/articles/optimize-inp): highly interactive pages should target Interaction to Next Paint of 200 ms or less at p75.
- [MDN: CSS container queries](https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_container_queries): component styling can respond to container size, not only viewport size.
- [MDN: content-visibility](https://developer.mozilla.org/en-US/docs/Web/CSS/content-visibility): offscreen rendering work can be skipped while preserving accessibility tree behavior when used correctly.
- [React: useTransition](https://react.dev/reference/react/useTransition): background rendering for non-urgent UI updates.
- [React: memo](https://react.dev/reference/react/memo): memoization is a performance optimization, not a semantic guarantee.
- [Tauri v2 window customization](https://v2.tauri.app/learn/window-customization/): custom title bars are supported, but can lose some native macOS window features.
- [Microsoft Research: Guidelines for Human-AI Interaction](https://www.microsoft.com/en-us/research/articles/guidelines-for-human-ai-interaction-eighteen-best-practices-for-human-centered-ai-design/): AI UX must keep users in control and respect goals and attention.
- [Microsoft Learn: Design foundations for agents](https://learn.microsoft.com/en-us/agents/design-guidelines/design-foundations): agent UX is a complete interaction system, not isolated prompts.
- [IBM Carbon for AI](https://carbondesignsystem.com/guidelines/carbon-for-ai/): AI styling should communicate AI involvement and explainability, not act as decoration.

## Current UI assessment

### What is already desktop-like

The application already has strong desktop foundations:

- Tauri shell, custom resize handles, title behavior, and native window calls in `src/components/Layout.tsx:978` and `src/components/AppBar.tsx:76`.
- File tabs with context menus and reorder behavior in `src/components/AppBar.tsx:127` and `src/components/AppBar.tsx:166`.
- Editor, terminal, file explorer, git panel, history panel, and chat as persistent work surfaces in `src/components/Layout.tsx:1174`.
- CodeMirror and xterm integration, which are appropriate desktop-class primitives for a coding tool.
- Lazy-loaded major panes in `src/components/Layout.tsx:44` through `src/components/Layout.tsx:49`.
- Local fonts and CSS variables in `src/styles/theme.css:1` and `src/styles/theme.css:43`.
- Some measured performance work, such as `recordDebugPerf`, requestAnimationFrame batching in `src/hooks/useChatV2.ts:922`, memoized chat rows in `src/chat/rendering/useChatTimelineRows.ts:41`, and memoized chat panel rendering in `src/chat/rendering/ChatPanel.tsx:182`.

These are worth preserving. The issue is not architecture alone. It is the product surface and interaction grammar.

### Where it reads web-first

The visual system uses web/SaaS vocabulary throughout:

- The theme names editor and panel surfaces as cards: `--bg-editor` is "Editor card surface" and `--bg-panel` is "Sidebar / chat cards" in `src/styles/theme.css:49` and `src/styles/theme.css:50`.
- Elevation is emphasized globally with large dark shadows in `src/styles/theme.css:156` through `src/styles/theme.css:160` and `--panel-shadow` in `src/styles/theme.css:175`.
- Glass is defined as a global utility in `src/styles/theme.css:168` and `src/index.css:194`, even though Fluent guidance treats acrylic-like materials as transient surfaces such as menus and popovers, not primary workspace structure.
- The main workspace is a set of floating rounded cards: activity bar `src/components/Layout.tsx:1040`, sidebar overlay `src/components/Layout.tsx:1121`, editor card `src/components/Layout.tsx:1177`, chat card `src/components/Layout.tsx:1308`, and rounded status bar `src/components/Layout.tsx:1399`.
- Empty states are brand/marketing-like: the editor welcome page uses a large logo, H1, large copy, and CTA stack in `src/components/EditorPanel.tsx:94` through `src/components/EditorPanel.tsx:145`; the chat empty state uses a centered card with logo, uppercase brand heading, and shadows in `src/chat/rendering/ChatViewport.tsx:157`.
- Component primitives default to cards and shadows: `Surface` has `card`, `elevated`, `danger`, and `modal` variants in `src/components/ui/Surface.tsx:10`; `ListRow` has a `card` variant with border, rounded corners, and shadow in `src/components/ui/ListRow.tsx:10`.
- The composer is styled as a nested rounded card inside a panel in `src/chat/composer/Composer.tsx:264` through `src/chat/composer/Composer.tsx:323`.

The result is polished, but it visually competes with the actual work. A desktop coding tool should feel like a stable instrument panel, not a collection of promotional blocks.

### Interaction and accessibility gaps

Several custom controls need desktop semantics and keyboard parity:

- Activity bar items are clickable `div`s with `onClick` in `src/components/Layout.tsx:1052`, `src/components/Layout.tsx:1068`, `src/components/Layout.tsx:1087`, and `src/components/Layout.tsx:1110`. They should be real `button`s or a proper toolbar/rail pattern.
- App tabs are draggable `div`s in `src/components/AppBar.tsx:319`. They should expose tab semantics, keyboard navigation, close behavior, and reorder alternatives.
- WCAG 2.2 requires a non-drag pointer alternative for drag operations. The tab context menu provides "move to beginning/end" in `src/components/AppBar.tsx:191`, which is useful, but there is no full adjacent move or keyboard reorder model.
- Some hit targets appear below the 24 by 24 CSS px minimum or are tightly packed, for example tab close button padding at `src/components/AppBar.tsx:352` and modal close/button sizing at `src/components/ui/Modal.tsx:81`.
- Custom menus and dialogs are visually present, but should be audited against WAI-ARIA APG patterns for menu, dialog, tablist, tree, listbox, combobox, and toolbar behavior.
- The custom title bar is legitimate in Tauri, but Tauri documentation notes that on macOS custom titlebars can lose native features. We should decide per platform whether full custom chrome is worth that cost.

### Performance gaps

The app is highly interactive, so responsiveness matters more than initial load alone.

- The new chat viewport maps all rows directly in `src/chat/rendering/ChatViewport.tsx:172`. The older chat panel has virtualization logic in `src/components/ChatPanel.tsx:435` onward. If ChatPanel V3 is the target, it needs equivalent virtualization or `content-visibility`/containment.
- We use many shadows and animated transitions on core surfaces. On WebKitGTK and embedded WebViews, these can add paint cost, especially with large rounded clipped panes.
- Smooth wheel handling in `src/hooks/useSmoothWheelScroll.ts` may improve perceived polish, but custom scroll physics can fight native platform behavior and must be measured.
- `ResizeObserver` in `src/components/Layout.tsx:948` updates state on width changes. It is useful, but should be watched for resize loops and unnecessary top-level re-rendering.
- React memoization exists, but per React guidance it is only an optimization, not a guarantee. We should validate with React Profiler and WebView traces rather than assuming memoized components solve the interaction budget.

## 2026 principles for this app

### 1. Desktop shell first, web renderer second

The app should follow desktop application conventions even though the renderer is HTML/CSS/React:

- Stable panes, not floating cards.
- Toolbars for view-local actions.
- Context menus for contextual commands, with menu bar or command palette equivalents for discoverability.
- Standard keyboard shortcuts.
- Persistent status bar and pane state.
- Native or platform-matched titlebar behavior where possible.

### 2. Dense does not mean cluttered

Coding, git, terminal, and agent review workflows are repeated work. The UI should optimize scan speed:

- 8 px base grid with 4 px sub-steps for compact regions.
- 24 px minimum target/spacing compliance for pointer targets.
- 28-32 px rows for file trees and lists.
- 32-36 px toolbars and tab strips.
- Text hierarchy based on weight, luminance, and alignment, not large headings.
- Minimal shadows in persistent chrome.

### 3. Panes, rails, bars, and overlays have different jobs

Use distinct primitives:

- `AppShell`: root app frame.
- `TitleToolbar`: window drag region, tabs, project identity, window controls.
- `ActivityRail`: narrow, persistent navigation rail.
- `Pane`: integrated editor/chat/sidebar surface with separators.
- `PanelHeader`: local title and actions.
- `StatusBar`: compact status and actionable indicators.
- `Popover`: transient menus and dropdowns.
- `Dialog`: blocking decisions.
- `Toast/InlineNotice`: nonblocking feedback.

Do not use one generic `Surface card` abstraction for all of these.

### 4. AI UX must emphasize control and review

For this product, "AI" is not decoration. It performs code changes and runs commands. Best practice is:

- Show what the agent is doing, what it changed, and what needs review.
- Provide stop, undo, accept/reject, inspect diff, and open-file actions close to the affected object.
- Avoid ambiguous "magic" states. Use explicit states: planning, editing, waiting for approval, running command, applying patch, needs review, failed, reverted.
- Make confidence and risk visible in human terms, not as decorative badges.
- Keep explanations available on demand, not always expanded.

### 5. Interaction performance is a product requirement

Use an app-local INP-equivalent target:

- p75 interaction-to-paint under 200 ms for common interactions.
- No text input frame drops in composer/editor.
- No visible lag opening sidebars, context menus, model dropdown, file tree, or command approval cards.
- No unbounded DOM growth in chat/history/tool output views.

## Recommended redesign direction

### A. Replace floating cards with integrated panes

Current:

- Activity rail, sidebar, editor, chat, and status bar all have border radius and shadows.
- The editor and chat look like separate cards placed on a web dashboard.

Target:

- Root frame uses one app background.
- App bar is flush to top.
- Activity rail is flush to left, with separators rather than shadows.
- Sidebar is docked by default when open, not floating over editor content.
- Editor and chat are integrated split panes separated by 1 px dividers and resize handles.
- Status bar is flush to bottom, not a rounded card.
- Only popovers, menus, dialogs, and command approval overlays use elevation.

Implementation sketch:

- Add tokens: `--surface-app`, `--surface-pane`, `--surface-toolbar`, `--surface-overlay`, `--separator`, `--focus-ring`, `--selection-bg`, `--row-hover`, `--row-selected`.
- Replace `--panel-shadow` on persistent panes with flat separators.
- Keep radius for popovers/dialogs only; persistent panes should be 0-4 px depending on platform and window state.
- Change comments and token names away from "card" language to "pane", "toolbar", "overlay", and "row".

### B. Redesign empty states as utility states

Current:

- Editor welcome page is a centered landing page with large logo, H1, large text, and CTA buttons.
- Chat empty state is a brand card.

Target:

- Empty editor area should prioritize "open a file", "open folder", "recent files", "start with current workspace", and configuration state.
- Chat empty state should be compact and workflow-oriented: input ready, recent task suggestions, current model state, missing configuration if any.
- Branding should be subtle in chrome, not central once inside the app.

Implementation sketch:

- Replace large editor welcome with a compact `StartWorkspacePanel` inside the editor pane.
- Replace chat empty card with a low-height inline state above the composer or a compact first-run checklist.
- Remove `text-3xl`, large logo, and marketing CTA stack from the primary app surface.

### C. Build desktop-grade command and keyboard model

Required commands:

- File: New File, Open Folder, Save, Save As, Close Tab, Close Others, Close All.
- View: Toggle Explorer, Toggle Git, Toggle History, Toggle Terminal, Focus Editor, Focus Chat, Focus Sidebar, Toggle Fullscreen.
- Edit/navigation: Find File, Find in File, Go to Symbol, Go to Line, Next/Previous Tab.
- Agent: New Conversation, Stop, Accept All, Reject All, Undo Tool, Open Last Diff, Approve Once, Deny.
- Help: Show Keyboard Shortcuts.

Implementation sketch:

- Create a command registry with id, label, shortcut, enablement, run handler, and menu group.
- Use it for menu bar, context menus, command palette, tooltips, and shortcuts.
- Align defaults with GNOME/Windows/macOS where possible: Ctrl+W close tab, Ctrl+Q quit on Linux/Windows, Ctrl+, settings, F9 side pane, F10 menu, Ctrl+? shortcuts, F11 fullscreen.
- On macOS, map display labels to Command-based shortcuts and consider using native menu support through Tauri where possible.

### D. Bring custom widgets up to APG patterns

Priority widgets:

- App tabs: `tablist`, `tab`, `tabpanel` where applicable, arrow-key navigation, close with middle click or command, keyboard close.
- Activity rail: buttons in a toolbar/rail with `aria-pressed` or selected state.
- File tree: verify tree roles, roving focus, arrow key expand/collapse, typeahead, context menu.
- Menus/context menus: menu/menuitem roles, Escape close, arrow navigation, focus restoration.
- Modals: focus trap, labelled dialog, inert background, initial focus, Escape behavior, return focus.
- Combobox/dropdowns: model selector, settings selectors, mention suggestions.
- Drag/reorder: provide keyboard and single-pointer alternatives for all drag behaviors.

Acceptance criteria:

- Full app can be operated with keyboard for core workflows.
- Focus ring is visible and not obscured.
- Pointer targets meet WCAG 2.2 minimum size or spacing.
- Accessibility smoke test with keyboard-only, screen reader quick pass, and axe where possible.

### E. Improve chat and timeline performance

Target:

- Chat V3 should not render unlimited message rows.
- Streaming should update the active row without triggering full timeline work.
- Long tool outputs should collapse, virtualize, or use `content-visibility`.

Implementation sketch:

- Port virtualization from legacy `src/components/ChatPanel.tsx` into `src/chat/rendering/ChatViewport.tsx`, or use a small tested virtualization library.
- Add `content-visibility: auto` and `contain-intrinsic-size` to long inactive message blocks where virtualization is not practical.
- Keep the active streaming message and recent tail unvirtualized.
- Split `ChatMessage` into smaller memoized units for markdown, tool output, approval card, and work log.
- Use React `startTransition` for non-urgent timeline/history/filter updates, never for controlled text input.
- Record interaction timing for opening sidebars, switching tabs, typing in composer, opening command palette, expanding tool output, and accepting/rejecting changes.

### F. Reduce paint cost in the design system

Actions:

- Remove shadows from persistent panes.
- Avoid backdrop blur or glass on large surfaces.
- Limit transition properties to `opacity`, `transform`, and color where needed.
- Avoid animating height for large regions; prefer transform for drawers or instant layout for panes.
- Audit `shadow-xl`, `drop-shadow`, `backdrop`, and large rounded clipped surfaces.
- Create a "reduced motion" path using `prefers-reduced-motion`.

### G. Make settings a desktop preferences surface

Current settings appear to mix cards, instructional blocks, large spacing, and promotional flows.

Target:

- Preferences window with left category list and right details pane.
- Compact controls with clear labels and inline help only where needed.
- Destructive or external-account flows in dialogs or focused subpanes.
- Consistent field, select, checkbox, segmented control, and button primitives.

### H. Use container queries for pane-local responsiveness

The app is split-pane based; viewport media queries are the wrong primary model. A 360 px chat pane and a 900 px editor pane can exist in the same window.

Actions:

- Put `container-type: inline-size` on major panes.
- Make chat header/composer/model selector adapt to pane width.
- Make git/file/history rows adapt to sidebar width.
- Make command approval cards compact in narrow chat and detailed in wide chat.

## Staged implementation plan

### Phase 0: Baseline and design audit (1-2 days)

Deliverables:

- Add screenshots of default, sidebar open, chat active, terminal open, settings, modal, and narrow window states.
- Add a simple interaction timing helper for app-local INP events.
- List all UI primitives and where each is used.
- Record current DOM size for chat/history with small, medium, and long sessions.
- Create an accessibility checklist for tabs, menus, dialogs, tree, combobox, rail, and drag/reorder.

Exit criteria:

- We know the current visual and performance baseline before changing it.

### Phase 1: Desktop design tokens and primitives (2-4 days)

Deliverables:

- Replace card-centric primitives with desktop primitives: `Pane`, `Toolbar`, `StatusBar`, `RailButton`, `PanelHeader`, `MenuSurface`, `DialogSurface`, `ListRow`, `Field`.
- Introduce flat pane tokens and separator tokens.
- Restrict shadows to popovers, menus, and dialogs.
- Add focus ring and target-size rules.
- Document usage rules in `docs/internal/UI_IMPROVEMENTS_ROADMAP.md` or a new design-system doc.

Exit criteria:

- New UI work cannot accidentally use the old card/elevation vocabulary for core workspace surfaces.

### Phase 2: App shell migration (3-6 days)

Deliverables:

- Convert activity rail `div`s to semantic buttons.
- Dock sidebar as a pane by default; keep overlay only for narrow windows if needed.
- Make editor/chat/status bar flush integrated panes.
- Remove rounded card treatment from persistent workspace surfaces.
- Preserve resize behavior and saved pane sizes.

Exit criteria:

- The app first impression reads as a desktop coding tool, not a web dashboard.

### Phase 3: Command model and keyboard parity (3-5 days)

Deliverables:

- Command registry.
- Unified shortcut handling.
- Command palette wired to registry.
- File/View/Agent/Help command groups.
- Keyboard shortcuts dialog.
- Context menus generated from command registry where practical.

Exit criteria:

- Every visible important command is discoverable in menu/palette/tooltip and available by keyboard where appropriate.

### Phase 4: Accessibility pass (4-7 days)

Deliverables:

- APG-compliant app tabs, activity rail, dialogs, menus, dropdowns, file tree, and mention suggestions.
- Non-drag alternatives for tab reorder and pane sizing where feasible.
- Focus management in dialogs/popovers.
- Target size audit.
- Keyboard-only test script.

Exit criteria:

- Core workflows can be completed keyboard-only: open file, switch tabs, use chat, approve/deny command, inspect diff, toggle terminal, open settings.

### Phase 5: Chat, history, and output performance (4-8 days)

Deliverables:

- Virtualized Chat V3 timeline or equivalent containment strategy.
- Virtualized/collapsed long tool outputs.
- Measured p75 interaction-to-paint for core interactions.
- React Profiler trace for chat streaming and tab switching.
- Reduced paint cost after visual migration.

Exit criteria:

- Long conversations remain responsive and do not cause full-surface re-render or scroll lag.

### Phase 6: AI workflow polish (4-8 days)

Deliverables:

- Explicit agent state model in UI.
- Review-focused change summaries.
- Clear risk states for command execution and auto-approve mode.
- Inline explanations on demand.
- Better accept/reject/undo/open-diff locality.

Exit criteria:

- Users can understand, interrupt, review, and recover from agent actions without reading raw logs.

## Specific issue backlog

High priority:

- Replace persistent pane shadows and rounded card shell in `Layout`.
- Convert activity rail items to buttons with selected state.
- Add command registry and keyboard shortcuts dialog.
- Make AppBar tabs semantic and keyboard navigable.
- Port virtualization/containment into ChatPanel V3.
- Replace editor and chat empty states with compact workflow states.
- Audit all close/icon buttons for 24 px target size.

Medium priority:

- Rework settings into a preferences-style layout.
- Normalize menu/context menu behavior through one primitive.
- Use container queries in chat/sidebar/editor chrome.
- Add reduced-motion styles.
- Audit model selector and mention suggestions against combobox/listbox patterns.
- Replace decorative AI badges/pills with stateful review/risk indicators.

Lower priority:

- Per-platform titlebar strategy: native/transparent on macOS, custom on Linux/Windows if needed.
- Theming refinement after structural migration.
- Optional native menu integration through Tauri.

## Non-goals

- Do not rewrite the app in a native UI framework.
- Do not remove React, Vite, Tailwind, or Tauri.
- Do not chase a single platform skin. This should be a cross-platform desktop app with platform-aware behavior.
- Do not remove density to make the UI "cleaner." Productivity density is a feature.
- Do not make the app look like VS Code exactly. Learn from desktop IDE patterns, but keep Zaguan Blade's identity.

## Success metrics

Visual/product:

- First screen communicates "desktop coding agent" without marketing-style hero or card layout.
- Persistent chrome uses panes and separators, not elevated cards.
- Work surfaces have clearer hierarchy and less visual competition.

Interaction:

- p75 app-local interaction-to-paint under 200 ms for common interactions.
- No dropped typing frames in editor or composer.
- Smooth chat streaming with long sessions.
- Sidebar, menus, and command palette open immediately.

Accessibility:

- Keyboard-only core workflow passes.
- Focus is visible and not obscured.
- Minimum target sizes/spacing pass WCAG 2.2 audit.
- Custom widgets follow APG keyboard expectations.

AI workflow:

- Users can see what the agent is doing.
- Users can stop, undo, inspect, accept, reject, and recover.
- AI states are meaningful, not decorative.

## Final recommendation

Keep the current architecture and Tauri/React foundation. Change the design language from "web dashboard with cards" to "desktop productivity workspace with panes." Start with the shell, tokens, and command model before polishing individual components. Once the shell stops looking like a collection of floating cards, the rest of the UI will have a much clearer path toward fast, accessible, desktop-grade behavior.
