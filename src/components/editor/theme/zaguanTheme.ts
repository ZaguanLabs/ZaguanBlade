import { EditorView } from "@codemirror/view";
import { Extension } from "@codemirror/state";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

// Zaguan Blade Theme - A premium dark theme with vibrant syntax colors
// Designed to match the surgical dark aesthetic of the app

// Tokyo Night Color Palette
// Matches the theme.css Tokyo Night colors for consistency
const colors = {
    // Base colors - Tokyo Night deep blue-grays
    bg: "#1f2335",           // Editor background (matches --bg-editor, slightly lighter for depth)
    bgPanel: "#16161e",      // Panel background (matches --bg-panel)
    bgSurface: "#1f202e",    // Surface background (matches --bg-surface)
    bgSurfaceHover: "#292a3a", // Hover state (matches --bg-surface-hover)
    
    // Foreground - Soft blue-tinted text
    fg: "#a9b1d6",           // Primary text (matches --fg-primary)
    fgMuted: "#787c99",      // Secondary text (matches --fg-secondary)
    fgSubtle: "#565f89",     // Tertiary text (matches --fg-tertiary)
    fgDim: "#3b4261",        // Dim text
    
    // Borders
    border: "#101014",       // Subtle borders (matches --border-subtle)
    borderFocus: "#3d59a1",  // Focus border (matches --border-focus)
    
    // Accent colors - Tokyo Night palette
    accent: "#7aa2f7",       // Blue - Primary accent (matches --accent-primary)
    accentBright: "#89b4fa", // Brighter blue
    
    // Syntax colors - Tokyo Night authentic palette
    keyword: "#bb9af7",      // Purple - Keywords, control flow
    string: "#9ece6a",       // Green - Strings
    number: "#ff9e64",       // Orange - Numbers
    comment: "#565f89",      // Muted blue-gray - Comments
    function: "#7aa2f7",     // Blue - Functions
    variable: "#c0caf5",     // Bright text - Variables
    type: "#2ac3de",         // Cyan - Types
    constant: "#ff9e64",     // Orange - Constants
    operator: "#89ddff",     // Light cyan - Operators
    property: "#73daca",     // Teal - Properties
    tag: "#f7768e",          // Red/pink - Tags (HTML/JSX)
    attribute: "#e0af68",    // Yellow - Attributes
    regexp: "#bb9af7",       // Purple - Regex
    macro: "#bb9af7",        // Purple - Macros
    
    // UI colors - Tokyo Night selection/highlight
    selection: "#515c7e4d",  // Selection (matches --bg-selection)
    selectionMatch: "#515c7e33",
    activeLine: "#292e42",   // Active line background
    activeLineGutter: "#1f2335",
    cursor: "#c0caf5",       // Bright cursor
    matchingBracket: "#515c7e80",
    
    // Gutter
    gutterBg: "transparent",
    gutterFg: "#3b4261",
    gutterActiveFg: "#787c99",
    
    // Line numbers
    lineNumber: "#3b4261",
    lineNumberActive: "#a9b1d6",
};

// Editor theme (UI styling)
export const zaguanEditorTheme = EditorView.theme({
    "&": {
        backgroundColor: colors.bg,
        color: colors.fg,
        fontSize: "14px",
        fontFamily: '"Fira Code", "Symbols Nerd Font Mono", monospace',
    },
    
    // Content area
    ".cm-content": {
        caretColor: colors.cursor,
        fontFamily: '"Fira Code", "Symbols Nerd Font Mono", monospace',
        lineHeight: "1.6",
        padding: "8px 0",
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
    
    // Active line
    ".cm-activeLine": {
        backgroundColor: colors.activeLine,
    },
    
    ".cm-activeLineGutter": {
        backgroundColor: colors.activeLineGutter,
    },
    
    // Gutters
    ".cm-gutters": {
        backgroundColor: colors.gutterBg,
        color: colors.gutterFg,
        border: "none",
        paddingRight: "8px",
    },
    
    ".cm-lineNumbers .cm-gutterElement": {
        color: colors.lineNumber,
        padding: "0 12px 0 8px",
        minWidth: "40px",
        fontFamily: '"Fira Code", monospace',
        fontSize: "12px",
    },
    
    ".cm-lineNumbers .cm-gutterElement.cm-activeLineGutter": {
        color: colors.lineNumberActive,
        fontWeight: "500",
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
        backgroundColor: "rgba(251, 191, 36, 0.3)",
        outline: "1px solid rgba(251, 191, 36, 0.6)",
        borderRadius: "2px",
    },
    
    ".cm-searchMatch.cm-searchMatch-selected": {
        backgroundColor: "rgba(251, 191, 36, 0.5)",
    },
    
    // Panels (search, etc.)
    ".cm-panels": {
        backgroundColor: colors.bgPanel,
        color: colors.fg,
        borderBottom: `1px solid ${colors.border}`,
    },
    
    ".cm-panels.cm-panels-top": {
        borderBottom: `1px solid ${colors.border}`,
    },
    
    ".cm-panels.cm-panels-bottom": {
        borderTop: `1px solid ${colors.border}`,
    },
    
    // Panel inputs
    ".cm-textfield": {
        backgroundColor: colors.bgSurface,
        color: colors.fg,
        border: `1px solid ${colors.border}`,
        borderRadius: "4px",
        padding: "4px 8px",
        fontSize: "13px",
        outline: "none",
    },
    
    ".cm-textfield:focus": {
        borderColor: colors.accent,
        boxShadow: `0 0 0 2px ${colors.selection}`,
    },
    
    ".cm-button": {
        backgroundColor: colors.bgSurface,
        color: colors.fg,
        border: `1px solid ${colors.border}`,
        borderRadius: "4px",
        padding: "4px 12px",
        fontSize: "13px",
        cursor: "pointer",
        transition: "all 0.15s ease",
    },
    
    ".cm-button:hover": {
        backgroundColor: colors.bgSurfaceHover,
        borderColor: colors.borderFocus,
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
            fontFamily: '"Fira Code", monospace',
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
        fontFamily: '"Fira Code", "Symbols Nerd Font Mono", monospace',
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
        fontFamily: '"Fira Code", monospace',
        fontSize: "12px",
        color: colors.fgMuted,
        marginBottom: "4px",
    },
    
    ".cm-zlp-location": {
        fontSize: "11px",
        color: colors.fgSubtle,
        marginTop: "6px",
    },
}, { dark: true });

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
    { tag: t.angleBracket, color: colors.fgMuted },
    
    // Attributes
    { tag: t.attributeName, color: colors.attribute },
    { tag: t.attributeValue, color: colors.string },
    
    // Punctuation
    { tag: t.punctuation, color: colors.fgMuted },
    { tag: t.separator, color: colors.fgMuted },
    { tag: t.bracket, color: colors.fgMuted },
    { tag: t.squareBracket, color: colors.fgMuted },
    { tag: t.paren, color: colors.fgMuted },
    { tag: t.brace, color: colors.fgMuted },
    
    // Meta
    { tag: t.meta, color: colors.fgMuted },
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
    { tag: t.monospace, fontFamily: '"Fira Code", monospace' },
    
    // Invalid
    { tag: t.invalid, color: "#ef4444", textDecoration: "underline wavy" },
]);

// Combined theme extension
export const zaguanTheme: Extension = [
    zaguanEditorTheme,
    syntaxHighlighting(zaguanHighlightStyle),
];

export default zaguanTheme;
