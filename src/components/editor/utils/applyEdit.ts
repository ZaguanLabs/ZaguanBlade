
import { EditorView } from "@codemirror/view";
import { LanguageWorkspaceEdit, LanguageTextEdit } from "../../../types/blade";

/**
 * Normalizes file paths for comparison.
 * Removes file:// prefix and normalizes separators.
 */
function normalizePath(path: string): string {
    let p = path.replace(/^file:\/\//, "");
    // Simple normalization for basic string matching
    // In a real app, might need more robust path handling
    return p;
}

/**
 * Applies a workspace edit to the current editor view if applicable.
 * Handles exact path matches and normalized path matches.
 * 
 * @param view - The CodeMirror editor view
 * @param currentFilePath - The absolute path of the currently open file
 * @param edit - The workspace edit to apply
 * @returns boolean - true if any changes were applied to the current view
 */
export function applyWorkspaceEdit(
    view: EditorView,
    currentFilePath: string,
    edit: LanguageWorkspaceEdit
): boolean {
    if (!edit.changes) return false;

    // Changes to apply to this view
    const transactionSpecs: { from: number; to: number; insert: string }[] = [];

    const normalizedCurrent = normalizePath(currentFilePath);

    for (const [path, changes] of Object.entries(edit.changes)) {
        const normalizedPath = normalizePath(path);

        // check for match
        if (normalizedPath === normalizedCurrent ||
            normalizedCurrent.endsWith(normalizedPath) ||
            normalizedPath.endsWith(normalizedCurrent)) {

            // Convert LSP edits (line/char 0-based) to CodeMirror changes (offset)
            const doc = view.state.doc;

            for (const change of changes) {
                try {
                    // Safe line access
                    const startLineNum = Math.min(doc.lines, Math.max(1, change.range.start.line + 1));
                    const endLineNum = Math.min(doc.lines, Math.max(1, change.range.end.line + 1));

                    const startLine = doc.line(startLineNum);
                    const endLine = doc.line(endLineNum);

                    // Safe character access
                    const startChar = Math.min(startLine.length, Math.max(0, change.range.start.character));
                    const endChar = Math.min(endLine.length, Math.max(0, change.range.end.character));

                    const from = startLine.from + startChar;
                    const to = endLine.from + endChar;

                    transactionSpecs.push({
                        from,
                        to,
                        insert: change.new_text
                    });
                } catch (e) {
                    console.error("[applyWorkspaceEdit] Failed to map range", change.range, e);
                }
            }
        } else {
            console.log(`[applyWorkspaceEdit] Skipping changes for ${path} (current: ${currentFilePath})`);
        }
    }

    if (transactionSpecs.length > 0) {
        view.dispatch({ changes: transactionSpecs });
        return true;
    }

    return false;
}
