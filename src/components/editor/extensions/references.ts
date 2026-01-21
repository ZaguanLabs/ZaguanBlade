/**
 * Find References Extension for CodeMirror
 * 
 * Implements "Find References" (Shift+F12).
 * Shows a menu of references if multiple are found, or jumps to the location if only one.
 */

import { Extension } from "@codemirror/state";
import { keymap, EditorView } from "@codemirror/view";
import { LanguageService } from "../../../services/language";
import type { LanguageLocation } from "../../../types/blade";

// Theme for references menu (reusing code actions theme styles largely)
const referencesTheme = EditorView.baseTheme({
    ".cm-references-menu": {
        backgroundColor: "var(--bg-secondary, #1e1e1e)",
        border: "1px solid var(--border-neutral, #444)",
        borderRadius: "6px",
        boxShadow: "0 4px 12px rgba(0,0,0,0.3)",
        padding: "4px 0",
        minWidth: "300px",
        maxWidth: "500px",
        maxHeight: "300px",
        overflowY: "auto",
        zIndex: "1000",
        fontFamily: "var(--font-sans, system-ui)"
    },
    ".cm-reference-item": {
        padding: "8px 12px",
        cursor: "pointer",
        display: "block",
        fontSize: "13px",
        color: "var(--text-primary, #e0e0e0)",
        borderBottom: "1px solid var(--border-neutral, #333)",
        "&:hover": {
            backgroundColor: "var(--bg-tertiary, #2a2a2a)"
        },
        "&:last-child": {
            borderBottom: "none"
        }
    },
    ".cm-reference-file": {
        fontWeight: "bold",
        marginBottom: "2px",
        color: "var(--accent-primary, #569cd6)"
    },
    ".cm-reference-line": {
        fontSize: "12px",
        color: "var(--text-secondary, #9e9e9e)",
        fontFamily: "var(--font-mono, monospace)"
    }
});

// Show references menu
function showReferencesMenu(
    view: EditorView,
    pos: number,
    locations: LanguageLocation[],
    onNavigate: (path: string, line: number, char: number) => void
) {
    // Remove any existing menu
    const existing = document.querySelector(".cm-references-menu");
    if (existing) existing.remove();

    // Create menu
    const menu = document.createElement("div");
    menu.className = "cm-references-menu";

    // Position near the cursor
    const coords = view.coordsAtPos(pos);
    if (coords) {
        menu.style.position = "fixed";
        menu.style.left = `${coords.left}px`;
        menu.style.top = `${coords.bottom + 4}px`;
    }

    // Add menu items
    for (const loc of locations) {
        const item = document.createElement("div");
        item.className = "cm-reference-item";

        const fileDiv = document.createElement("div");
        fileDiv.className = "cm-reference-file";
        // Show relative path or filename
        const parts = loc.file_path.split('/');
        fileDiv.textContent = parts[parts.length - 1] + (parts.length > 1 ? ` (${loc.file_path})` : "");
        item.appendChild(fileDiv);

        const lineDiv = document.createElement("div");
        lineDiv.className = "cm-reference-line";
        lineDiv.textContent = `Line ${loc.range.start.line + 1}:${loc.range.start.character + 1}`;
        item.appendChild(lineDiv);

        item.onclick = () => {
            menu.remove();
            onNavigate(loc.file_path, loc.range.start.line, loc.range.start.character);
        };

        menu.appendChild(item);
    }

    // Add close handler
    const closeHandler = (e: MouseEvent) => {
        if (!menu.contains(e.target as Node)) {
            menu.remove();
            document.removeEventListener("click", closeHandler);
        }
    };
    setTimeout(() => document.addEventListener("click", closeHandler), 0);

    document.body.appendChild(menu);
}

export function referencesExtension(
    filePath: string,
    onNavigate: (path: string, line: number, character: number) => void
): Extension {
    return [
        referencesTheme,
        keymap.of([{
            key: "Shift-F12",
            run: (view) => {
                const pos = view.state.selection.main.head;
                const line = view.state.doc.lineAt(pos);
                const lineNum = line.number - 1;
                const char = pos - line.from;

                LanguageService.getReferences(filePath, lineNum, char, true)
                    .then(locations => {
                        if (locations.length === 0) {
                            // TODO: Show toast "No references found"
                            console.log("No references found");
                        } else if (locations.length === 1) {
                            // Jump directly
                            const loc = locations[0];
                            onNavigate(loc.file_path, loc.range.start.line, loc.range.start.character);
                        } else {
                            // Show menu
                            showReferencesMenu(view, pos, locations, onNavigate);
                        }
                    })
                    .catch(e => console.error("References lookup failed", e));

                return true;
            }
        }])
    ];
}
