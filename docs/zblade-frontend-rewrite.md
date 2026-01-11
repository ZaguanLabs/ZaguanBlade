# Zaguán Blade Frontend Rewrite & UI Redesign Plan

This document outlines the strategy for migrating the Zaguán Blade frontend from Next.js to a Vite + React + React Router stack, while simultaneously implementing a robust design system to facilitate future theming and improved aesthetics.

## 1. Objective

1.  **Stack Migration:** Replace Next.js with a standard Vite Single Page Application (SPA) setup.
2.  **UI Redesign:** Establish a clear, token-based design system (colors, spacing, typography) to decouple the UI from hardcoded values, enabling easy theming.
3.  **Feature Parity:** Strictly maintain all existing functionality (Terminal, Chat v1.1 Protocol, Streaming, File Ops) while improving the visual presentation.

**Target Stack:**
- **Build Tool:** Vite
- **Framework:** React 19
- **Routing:** React Router v7
- **State/Logic:** Custom Hooks (Preserved/Refactored)
- **Styling:** Tailwind CSS v4 + CSS Variables (Theming Engine)
- **Icons:** Lucide React

## 2. Rationale

- **Tauri Alignment:** Vite SPAs are the gold standard for Tauri apps, offering better performance and simpler builds than Next.js SSG/SSR.
- **Theming & Aesthetics:** The current UI relies on hardcoded Tailwind classes (e.g., `bg-zinc-900`). Moving to semantic CSS variables (e.g., `--bg-layer-1`) allows for instant theme switching and a more "premium" feel.
- **Maintainability:** A clear separation of design tokens from component logic will make the codebase easier to maintain.

## 3. Design System Strategy

We will adopt a **semantic token approach** using CSS variables. This abstracts the *value* of a color from its *purpose*.

### 3.1. Color Tokens (Theme Agnostic)
The application will use semantic names for all colors.
*   `--bg-app`: Root background
*   `--bg-panel`: Secondary backgrounds (sidebars, cards)
*   --bg-surface`: Interactive elements (inputs, buttons)
*   `--fg-primary`: Main text
*   `--fg-secondary`: Muted text/metadata
*   `--border-subtle`: Dividers
*   `--border-focus`: Active states
*   `--accent-primary`: Main brand/action color
*   `--status-success`, `--status-error`, `--status-warning`: Functional colors

### 3.2. Typography
*   **Sans:** Inter or similar variable font for UI.
*   **Mono:** JetBrains Mono (or similar) for code/terminal.

## 4. Implementation Steps

### Phase 1: Environment & Dependencies (Clean Slate)
1.  **Backup:** Ensure git status is clean and create `feature/vite-migration`.
2.  **Uninstall Next.js:** Remove `next`, `next-intl`, related eslint plugins.
3.  **Install Vite Ecosystem:**
    ```bash
    pnpm add -D vite @vitejs/plugin-react
    pnpm add react-router-dom react-i18next i18next i18next-http-backend i18next-browser-languagedetector klass (for class merging)
    ```
4.  **Clean Config:** Remove `next.config.ts`, `next-env.d.ts`.

### Phase 2: Design System Foundation (The "Bones")
1.  **Create `src/styles/theme.css`**: Define the root CSS variables for the default "Surgical Dark" theme.
2.  **Refactor `src/index.css`**: Import `theme.css` and setup Tailwind directives.
3.  **Tailwind Config:** Configure Tailwind (v4 or config file) to map utilities to these variables (e.g., `bg-app` -> `var(--bg-app)`).
4.  **Global Reset:** Ensure consistent box-sizing and font rendering.

### Phase 3: Structural Migration (The "Skeleton")
1.  **Vite Setup:** Create `vite.config.ts` and `index.html`.
2.  **Entry Point (`src/main.tsx`)**: Setup `BrowserRouter` and `I18nextProvider`.
3.  **App Root (`src/App.tsx`)**:
    - Recreate the main layout shell using the new Design System tokens.
    - Implement the Split-Pane layout (if applicable) or main grid.

### Phase 4: Component Refactoring & Redesign (The "Flesh")
*Migrate components one by one, replacing hardcoded styles with design tokens.*

1.  **Core UI Components (New)**:
    - Create `src/components/ui/Button.tsx`: Variants for primary, ghost, icon.
    - Create `src/components/ui/Input.tsx`: Standard text inputs.
    - Create `src/components/ui/ScrollArea.tsx`: Custom scrollbars are crucial for "premium" feel.
2.  **Terminal Component**:
    - Refactor `src/components/Terminal.tsx`.
    - Ensure it uses `TerminalBuffer` and v1.1 logic.
    - Update container styling to use `--bg-terminal`, `--font-mono`.
3.  **Chat Interface**:
    - Refactor `src/components/Chat.tsx` (or equivalent).
    - Ensure `MessageBuffer`, auto-scroll, and markdown rendering work.
    - Style chunks (user vs agent) using design tokens.
4.  **Remove Next.js Specifics**:
    - `next/link` -> `react-router-dom` Link.
    - `next/image` -> `img` tag.

### Phase 5: Functionality Re-Integration
1.  **Tauri Events:** Ensure `useBlade` and other custom hooks are correctly initialized in `App.tsx`.
2.  **Window Controls:** If custom titlebar exists, update it to new design.
3.  **Shortcuts/Keybindings:** Verify global listeners still attach to `window`/`document`.

### Phase 6: Polish & Verification
1.  **Theme Check:** Verify all UI elements act correctly to the defined variables.
2.  **Protocol Check:** Verify Chat/Terminal streams work with Sequence numbers and Idempotency (v1.1 verified).
3.  **Build:** Run `pnpm tauri build` to ensure the release asset is generated correctly.

## 5. Execution Plan
- [ ] **Setup:** Install Vite, clean Next.js, create `vite.config.ts`.
- [ ] **Design:** Create `src/styles/theme.css` with initial palette.
- [ ] **Structure:** Scaffold `main.tsx` and `App.tsx`.
- [ ] **Migration - Core:** Migrate `i18n` and global providers.
- [ ] **Migration - Components:** Rewrite components using new styling tokens.
- [ ] **Integration:** Re-connect logic hooks (`useChat`, `useTerminal`).
- [ ] **Verify:** Test full flow (Chat -> Terminal -> Edit).
