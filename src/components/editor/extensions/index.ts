// Re-export all custom extensions
export { indentGuides } from "./indentGuides";
export { rainbowBrackets } from "./rainbowBrackets";
export { smoothCursor } from "./smoothCursor";
export { scrollPastEnd } from "./scrollPastEnd";
export { lineHighlightField, addLineHighlight, clearLineHighlight } from "./lineHighlight";
export { virtualBufferField, setBaseContent, getVirtualContent, hasVirtualChanges } from "./virtualBuffer";
export { diffDecorations, diffStateField, setDiffState, clearDiff, parseUnifiedDiff, createDiffStateFromUnifiedDiff, aiGlowDecorations, triggerAiGlow, clearAiGlow, type DiffState } from "./diffDecorations";
export { zlpHoverTooltip } from "./zlpTooltip";
