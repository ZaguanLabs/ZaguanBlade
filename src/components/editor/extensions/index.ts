// Re-export all custom extensions
export { indentGuides } from "./indentGuides";
export { rainbowBrackets } from "./rainbowBrackets";
export { smoothCursor } from "./smoothCursor";
export { scrollPastEnd } from "./scrollPastEnd";
export { lineHighlightField, addLineHighlight, clearLineHighlight } from "./lineHighlight";
export { diffsField, addDiff, removeDiff, clearDiffs, acceptDiff, rejectDiff } from "./diffView";
export { virtualBufferField, setBaseContent, getVirtualContent, hasVirtualChanges } from "./virtualBuffer";
export {
    inlineDiffField,
    inlineDiffTheme,
    setInlineDiff,
    clearInlineDiff,
    computeDiffLines,
    type PendingInlineDiff
} from "./inlineDiff";
export { languageFeatures } from "./languageFeatures";
export { diagnosticsExtension, setDiagnostics, clearDiagnostics, getDiagnostics } from "./diagnostics";
export { signatureHelpExtension, triggerSignatureHelp } from "./signatureHelp";
export { codeActionsExtension, requestCodeActions } from "./codeActions";
export { referencesExtension } from "./references";
