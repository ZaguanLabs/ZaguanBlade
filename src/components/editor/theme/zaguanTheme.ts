import { EditorView } from "@codemirror/view";
import { Extension } from "@codemirror/state";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { classHighlighter, tags as t } from "@lezer/highlight";

const EDITOR_MONO_STACK = '"JetBrains Mono", "Cascadia Mono", "SFMono-Regular", "Consolas", "Liberation Mono", monospace';

// Zaguan Blade Theme — Intentional "dimmed structural" palette with selective pop colors.
// Structural tokens (keywords, operators, punctuation) recede; data tokens (strings,
// numbers, functions, types) pop. Matches the OLED-dark spatial UI theme.

const colors = {
    // Base — OLED-dark, unified with app theme
    bg: "var(--editor-bg)",
    bgPanel: "var(--editor-bg-panel)",
    bgSurface: "var(--editor-bg-surface)",
    bgSurfaceHover: "var(--editor-bg-surface-hover)",

    // Foreground — neutral, no blue tint
    fg: "var(--editor-fg)",
    fgMuted: "var(--editor-fg-muted)",
    fgSubtle: "var(--editor-fg-subtle)",
    fgDim: "var(--editor-fg-dim)",

    // Borders
    border: "var(--editor-border)",
    borderFocus: "var(--editor-border-focus)",

    // Accent — electric violet
    accent: "var(--editor-accent)",

    // Syntax — dimmed structural
    keyword: "var(--syntax-keyword)",
    operator: "var(--syntax-operator)",
    punctuation: "var(--syntax-punctuation)",
    variable: "var(--syntax-variable)",
    comment: "var(--syntax-comment)",

    // Syntax — pop (data tokens)
    string: "var(--syntax-string)",
    number: "var(--syntax-number)",
    function: "var(--syntax-function)",
    type: "var(--syntax-type)",
    constant: "var(--syntax-constant)",
    regexp: "var(--syntax-regexp)",
    macro: "var(--syntax-macro)",
    property: "var(--syntax-property)",
    tag: "var(--syntax-tag)",
    attribute: "var(--syntax-attribute)",

    // UI
    selection: "var(--editor-selection)",
    selectionMatch: "var(--editor-selection-match)",
    cursor: "var(--editor-cursor)",
    matchingBracket: "var(--editor-matching-bracket)",

    // Gutter
    gutterBg: "var(--editor-bg)",
    lineNumber: "var(--editor-line-number)",
    lineNumberActive: "var(--editor-line-number-active)",
};

// Editor theme (UI styling)
const zaguanEditorThemeSpec = {
    "&": {
        backgroundColor: colors.bg,
        color: colors.fg,
        fontSize: "var(--editor-content-font-size, 14px)",
        fontFamily: EDITOR_MONO_STACK,
    },
    
    // Content area
    ".cm-content": {
        caretColor: colors.cursor,
        fontFamily: EDITOR_MONO_STACK,
        lineHeight: "1.6",
        padding: "12px 0",
    },
    
    // Cursor styling
    ".cm-cursor, .cm-dropCursor": {
        borderLeftColor: colors.cursor,
        borderLeftWidth: "2px",
    },
    
    // Selection
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
        backgroundColor: colors.selection,
    },
    
    ".cm-selectionMatch": {
        backgroundColor: colors.selectionMatch,
        borderRadius: "2px",
    },
    
    // Active line — subtle top/bottom inset lines instead of a solid box
    ".cm-activeLine": {
        backgroundColor: "transparent",
        boxShadow: "inset 0 1px 0 rgba(255,255,255,0.04), inset 0 -1px 0 rgba(255,255,255,0.04)",
    },
    
    ".cm-activeLineGutter": {
        backgroundColor: "transparent",
    },
    
    // Gutters
    ".cm-gutters": {
        backgroundColor: colors.gutterBg,
        color: colors.lineNumber,
        border: "none",
        paddingRight: "4px",
    },
    
    ".cm-lineNumbers .cm-gutterElement": {
        color: colors.lineNumber,
        opacity: "0.35",
        padding: "0 6px 0 6px",
        minWidth: "32px",
        fontFamily: EDITOR_MONO_STACK,
        fontSize: "calc(var(--editor-content-font-size, 14px) - 2px)",
    },
    
    ".cm-lineNumbers .cm-gutterElement.cm-activeLineGutter": {
        color: colors.lineNumberActive,
        opacity: "1",
        fontWeight: "600",
    },
    
    // Fold gutter
    ".cm-foldGutter .cm-gutterElement": {
        color: colors.fgSubtle,
        padding: "0 4px",
        cursor: "pointer",
        transition: "color 0.15s ease",
    },
    
    ".cm-foldGutter .cm-gutterElement:hover": {
        color: colors.fg,
    },
    
    // Fold placeholder
    ".cm-foldPlaceholder": {
        backgroundColor: colors.bgSurface,
        color: colors.fgMuted,
        border: `1px solid ${colors.border}`,
        borderRadius: "4px",
        padding: "0 6px",
        margin: "0 4px",
        fontSize: "12px",
    },
    
    // Matching brackets
    "&.cm-focused .cm-matchingBracket": {
        backgroundColor: colors.matchingBracket,
        outline: `1px solid ${colors.accent}`,
        borderRadius: "2px",
    },
    
    "&.cm-focused .cm-nonmatchingBracket": {
        backgroundColor: "rgba(239, 68, 68, 0.3)",
        outline: "1px solid rgba(239, 68, 68, 0.6)",
    },
    
    // Search
    ".cm-searchMatch": {
        backgroundColor: "color-mix(in srgb, var(--accent-warning) 28%, transparent)",
        outline: "1px solid color-mix(in srgb, var(--accent-warning) 62%, transparent)",
        borderRadius: "4px",
    },
    
    ".cm-searchMatch.cm-searchMatch-selected": {
        backgroundColor: "color-mix(in srgb, var(--accent-warning) 48%, transparent)",
        outlineColor: "var(--accent-warning)",
    },
    
    // Panels (search, etc.)
    ".cm-panels": {
        backgroundColor: colors.bgPanel,
        color: colors.fg,
        borderBottom: `1px solid ${colors.border}`,
        fontFamily: "var(--font-sans)",
    },
    
    ".cm-panels.cm-panels-top": {
        borderBottom: `1px solid ${colors.border}`,
    },
    
    ".cm-panels.cm-panels-bottom": {
        borderTop: `1px solid ${colors.border}`,
    },

    ".cm-panel.cm-search": {
        display: "flex",
        alignItems: "center",
        flexWrap: "wrap",
        gap: "8px",
        padding: "10px 12px",
        backgroundColor: "color-mix(in srgb, var(--bg-panel) 94%, var(--accent-ai) 6%)",
        borderBottom: `1px solid ${colors.border}`,
        boxShadow: "var(--shadow-sm)",
        fontFamily: "var(--font-sans)",
        fontSize: "12px",
        letterSpacing: "0",
    },

    ".cm-panel.cm-search > *": {
        margin: "0",
    },

    ".cm-panel.cm-search label": {
        display: "inline-flex",
        alignItems: "center",
        gap: "5px",
        minHeight: "28px",
        color: colors.fgMuted,
        fontSize: "11px",
        fontWeight: "500",
        letterSpacing: "0.01em",
        whiteSpace: "nowrap",
        userSelect: "none",
    },
    
    // Panel inputs
    ".cm-textfield": {
        backgroundColor: colors.bgSurface,
        color: colors.fg,
        border: `1px solid ${colors.border}`,
        borderRadius: "calc(var(--panel-radius) * 0.55)",
        padding: "6px 10px",
        fontFamily: "var(--font-sans)",
        fontSize: "12px",
        fontWeight: "500",
        lineHeight: "1.35",
        outline: "none",
        minHeight: "30px",
        boxShadow: "inset 0 1px 0 color-mix(in srgb, var(--fg-bright) 4%, transparent)",
        transition: "border-color var(--transition-fast), box-shadow var(--transition-fast), background-color var(--transition-fast)",
    },

    ".cm-panel.cm-search .cm-textfield": {
        minWidth: "220px",
        maxWidth: "min(42vw, 360px)",
    },
    
    ".cm-textfield:focus": {
        borderColor: colors.accent,
        backgroundColor: "color-mix(in srgb, var(--bg-surface) 92%, var(--accent-ai) 8%)",
        boxShadow: `0 0 0 3px color-mix(in srgb, ${colors.accent} 18%, transparent)`,
    },
    
    ".cm-button": {
        backgroundColor: "color-mix(in srgb, var(--bg-surface) 92%, var(--fg-primary) 8%)",
        color: colors.fg,
        border: `1px solid ${colors.border}`,
        borderRadius: "calc(var(--panel-radius) * 0.55)",
        padding: "6px 10px",
        fontFamily: "var(--font-sans)",
        fontSize: "11px",
        fontWeight: "650",
        lineHeight: "1.35",
        cursor: "pointer",
        minHeight: "30px",
        boxShadow: "var(--shadow-sm)",
        transition: "background-color var(--transition-fast), border-color var(--transition-fast), color var(--transition-fast), transform var(--transition-fast), box-shadow var(--transition-fast)",
    },
    
    ".cm-button:hover": {
        backgroundColor: colors.bgSurfaceHover,
        borderColor: colors.borderFocus,
        color: "var(--fg-bright)",
        transform: "translateY(-1px)",
        boxShadow: "var(--shadow-md)",
    },

    ".cm-button:active": {
        transform: "translateY(0)",
        boxShadow: "var(--shadow-sm)",
    },

    ".cm-panel.cm-search .cm-button[name='next'], .cm-panel.cm-search .cm-button[name='prev']": {
        backgroundColor: "color-mix(in srgb, var(--accent-ai) 16%, var(--bg-surface) 84%)",
        borderColor: "color-mix(in srgb, var(--accent-ai) 32%, var(--border-subtle) 68%)",
        color: colors.fg,
    },

    ".cm-panel.cm-search .cm-button[name='next']:hover, .cm-panel.cm-search .cm-button[name='prev']:hover": {
        backgroundColor: "color-mix(in srgb, var(--accent-ai) 26%, var(--bg-surface) 74%)",
        borderColor: colors.accent,
        color: "var(--fg-bright)",
    },

    ".cm-panel.cm-search .cm-button[name='close']": {
        marginLeft: "auto",
        width: "30px",
        padding: "0",
        borderRadius: "999px",
        color: colors.fgMuted,
        backgroundColor: "transparent",
        borderColor: "transparent",
        boxShadow: "none",
        fontSize: "16px",
        fontWeight: "500",
    },

    ".cm-panel.cm-search .cm-button[name='close']:hover": {
        backgroundColor: "color-mix(in srgb, var(--state-danger) 16%, transparent)",
        borderColor: "color-mix(in srgb, var(--state-danger) 28%, transparent)",
        color: "var(--state-danger)",
        boxShadow: "none",
    },

    ".cm-panel.cm-search input[type='checkbox']": {
        width: "13px",
        height: "13px",
        margin: "0",
        accentColor: colors.accent,
        cursor: "pointer",
    },

    ".cm-panel.cm-search [name='matchCase'], .cm-panel.cm-search [name='regexp'], .cm-panel.cm-search [name='wholeWord']": {
        marginLeft: "2px",
    },
    
    // Tooltips
    ".cm-tooltip": {
        backgroundColor: colors.bgPanel,
        color: colors.fg,
        border: `1px solid ${colors.border}`,
        borderRadius: "6px",
        boxShadow: "0 4px 20px -2px rgba(0, 0, 0, 0.5)",
    },
    
    ".cm-tooltip.cm-tooltip-autocomplete": {
        "& > ul": {
            fontFamily: EDITOR_MONO_STACK,
            fontSize: "13px",
        },
        "& > ul > li": {
            padding: "4px 12px",
        },
        "& > ul > li[aria-selected]": {
            backgroundColor: colors.bgSurfaceHover,
            color: colors.fg,
        },
    },
    
    // Autocomplete icons
    ".cm-completionIcon": {
        opacity: "0.8",
        paddingRight: "8px",
    },
    
    ".cm-completionLabel": {
        color: colors.fg,
    },
    
    ".cm-completionDetail": {
        color: colors.fgMuted,
        fontStyle: "italic",
        marginLeft: "8px",
    },
    
    ".cm-completionMatchedText": {
        color: colors.accent,
        fontWeight: "600",
        textDecoration: "none",
    },
    
    // Lint
    ".cm-lintRange-error": {
        backgroundImage: `url("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='6' height='3'><path d='m0 3 l2 -2 l1 0 l2 2 l1 0' stroke='%23ef4444' fill='none' stroke-width='1'/></svg>")`,
    },
    
    ".cm-lintRange-warning": {
        backgroundImage: `url("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='6' height='3'><path d='m0 3 l2 -2 l1 0 l2 2 l1 0' stroke='%23f59e0b' fill='none' stroke-width='1'/></svg>")`,
    },
    
    ".cm-lintRange-info": {
        backgroundImage: `url("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='6' height='3'><path d='m0 3 l2 -2 l1 0 l2 2 l1 0' stroke='%2360a5fa' fill='none' stroke-width='1'/></svg>")`,
    },
    
    // Indent guides (via CSS)
    ".cm-line": {
        position: "relative",
    },
    
    // Scroller
    ".cm-scroller": {
        overflow: "auto",
        fontFamily: EDITOR_MONO_STACK,
    },
    
    // ZLP Hover Tooltip
    ".cm-zlp-tooltip": {
        padding: "8px 12px",
        maxWidth: "400px",
        fontSize: "13px",
        lineHeight: "1.5",
    },
    
    ".cm-zlp-kind": {
        display: "inline-block",
        padding: "2px 6px",
        backgroundColor: colors.accent,
        color: colors.bg,
        borderRadius: "3px",
        fontSize: "11px",
        fontWeight: "600",
        textTransform: "uppercase",
        marginBottom: "6px",
    },
    
    ".cm-zlp-name": {
        fontWeight: "600",
        fontSize: "14px",
        marginBottom: "4px",
        color: colors.fg,
    },
    
    ".cm-zlp-signature": {
        fontFamily: EDITOR_MONO_STACK,
        fontSize: "12px",
        color: colors.fgMuted,
        marginBottom: "4px",
    },
    
    ".cm-zlp-location": {
        fontSize: "11px",
        color: colors.fgSubtle,
        marginTop: "6px",
    },
};

export const zaguanEditorThemeDark = EditorView.theme(zaguanEditorThemeSpec, { dark: true });
export const zaguanEditorThemeLight = EditorView.theme(zaguanEditorThemeSpec, { dark: false });

// Syntax highlighting
export const zaguanHighlightStyle = HighlightStyle.define([
    // Comments
    { tag: t.comment, color: colors.comment, fontStyle: "italic" },
    { tag: t.lineComment, color: colors.comment, fontStyle: "italic" },
    { tag: t.blockComment, color: colors.comment, fontStyle: "italic" },
    { tag: t.docComment, color: colors.comment, fontStyle: "italic" },
    
    // Keywords
    { tag: t.keyword, color: colors.keyword, fontWeight: "500" },
    { tag: t.controlKeyword, color: colors.keyword, fontWeight: "500" },
    { tag: t.moduleKeyword, color: colors.keyword, fontWeight: "500" },
    { tag: t.operatorKeyword, color: colors.keyword },
    { tag: t.definitionKeyword, color: colors.keyword, fontWeight: "500" },
    
    // Operators
    { tag: t.operator, color: colors.operator },
    { tag: t.compareOperator, color: colors.operator },
    { tag: t.arithmeticOperator, color: colors.operator },
    { tag: t.logicOperator, color: colors.operator },
    { tag: t.bitwiseOperator, color: colors.operator },
    { tag: t.updateOperator, color: colors.operator },
    { tag: t.derefOperator, color: colors.operator },
    
    // Strings
    { tag: t.string, color: colors.string },
    { tag: t.special(t.string), color: colors.string },
    { tag: t.docString, color: colors.string },
    { tag: t.character, color: colors.string },
    { tag: t.escape, color: colors.regexp },
    
    // Numbers
    { tag: t.number, color: colors.number },
    { tag: t.integer, color: colors.number },
    { tag: t.float, color: colors.number },
    
    // Boolean & null
    { tag: t.bool, color: colors.constant },
    { tag: t.null, color: colors.constant },
    
    // Variables
    { tag: t.variableName, color: colors.variable },
    { tag: t.definition(t.variableName), color: colors.variable },
    { tag: t.local(t.variableName), color: colors.variable },
    { tag: t.special(t.variableName), color: colors.constant },
    
    // Functions
    { tag: t.function(t.variableName), color: colors.function },
    { tag: t.definition(t.function(t.variableName)), color: colors.function },
    
    // Properties
    { tag: t.propertyName, color: colors.property },
    { tag: t.definition(t.propertyName), color: colors.property },
    { tag: t.special(t.propertyName), color: colors.property },
    
    // Types
    { tag: t.typeName, color: colors.type },
    { tag: t.className, color: colors.type },
    { tag: t.namespace, color: colors.type },
    { tag: t.standard(t.typeName), color: colors.type },
    
    // Constants
    { tag: t.constant(t.variableName), color: colors.constant },
    
    // Labels
    { tag: t.labelName, color: colors.accent },
    
    // Regex
    { tag: t.regexp, color: colors.regexp },
    
    // Tags (HTML/JSX)
    { tag: t.tagName, color: colors.tag },
    { tag: t.standard(t.tagName), color: colors.tag },
    { tag: t.angleBracket, color: colors.punctuation },
    
    // Attributes
    { tag: t.attributeName, color: colors.attribute },
    { tag: t.attributeValue, color: colors.string },
    
    // Punctuation
    { tag: t.punctuation, color: colors.punctuation },
    { tag: t.separator, color: colors.punctuation },
    { tag: t.bracket, color: colors.punctuation },
    { tag: t.squareBracket, color: colors.punctuation },
    { tag: t.paren, color: colors.punctuation },
    { tag: t.brace, color: colors.punctuation },
    
    // Meta
    { tag: t.meta, color: colors.fgSubtle },
    { tag: t.annotation, color: colors.macro },
    { tag: t.processingInstruction, color: colors.macro },
    
    // Macros (Rust)
    { tag: t.macroName, color: colors.macro },
    
    // Headings (Markdown)
    { tag: t.heading, color: colors.accent, fontWeight: "600" },
    { tag: t.heading1, color: colors.accent, fontWeight: "700", fontSize: "1.4em" },
    { tag: t.heading2, color: colors.accent, fontWeight: "600", fontSize: "1.2em" },
    { tag: t.heading3, color: colors.accent, fontWeight: "600" },
    
    // Links
    { tag: t.link, color: colors.accent, textDecoration: "underline" },
    { tag: t.url, color: colors.accent },
    
    // Emphasis
    { tag: t.emphasis, fontStyle: "italic" },
    { tag: t.strong, fontWeight: "bold" },
    { tag: t.strikethrough, textDecoration: "line-through" },
    
    // Code
    { tag: t.monospace, fontFamily: EDITOR_MONO_STACK },
    
    // Invalid
    { tag: t.invalid, color: "#ef4444", textDecoration: "underline wavy" },
]);

// AI glow keyframe — injected as a base theme so CM manages it
export const zaguanGlowTheme = EditorView.baseTheme({
    "@keyframes cm-ai-glow-fade": {
        "0%": { textShadow: "0 0 8px rgba(99,102,241,0.85), 0 0 20px rgba(99,102,241,0.4)" },
        "100%": { textShadow: "none" },
    },
    ".cm-ai-appeared": {
        animation: "cm-ai-glow-fade 2s ease-out forwards",
    },
});

// Combined theme extension
export function getZaguanTheme(isDark: boolean): Extension {
    return [
        isDark ? zaguanEditorThemeDark : zaguanEditorThemeLight,
        zaguanGlowTheme,
        syntaxHighlighting(zaguanHighlightStyle),
        syntaxHighlighting(classHighlighter),
    ];
}
