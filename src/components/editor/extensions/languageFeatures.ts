
import { Extension } from "@codemirror/state";
import {
    autocompletion,
    CompletionContext,
    CompletionResult,
    Completion
} from "@codemirror/autocomplete";
import { hoverTooltip } from "@codemirror/view";
import { LanguageService } from "../../../services/language";

function createCompletionSource(filePath: string) {
    return async (context: CompletionContext): Promise<CompletionResult | null> => {
        // Trigger on "explicit" (Ctrl+Space) or if typing words/dots
        const word = context.matchBefore(/[\w\.]*$/);

        // If no word match and not explicit, don't trigger (unless trigger characters?)
        // LSP usually handles trigger characters.
        if (!context.explicit && (!word || word.from === word.to)) return null;

        const pos = context.pos;
        const line = context.state.doc.lineAt(pos);
        const lineNum = line.number - 1; // 0-based
        const char = pos - line.from; // 0-based

        try {
            const items = await LanguageService.getCompletions(filePath, lineNum, char);

            if (items.length === 0) return null;

            const options: Completion[] = items.map(item => ({
                label: item.label,
                type: mapKind(item.kind),
                detail: item.detail || undefined,
                info: item.documentation || undefined,
                apply: item.insert_text || item.label,
                boost: item.label.startsWith("_") ? -1 : 0 // Deprioritize internal symbols example
            }));

            return {
                from: word ? word.from : context.pos,
                options,
                // Valid for identifier characters and dots
                validFor: /^[\w\.]*$/
            };
        } catch (e) {
            console.error("Autocompletion failed", e);
            return null;
        }
    };
}

function createHoverTooltip(filePath: string) {
    return hoverTooltip(async (view, pos, side) => {
        const line = view.state.doc.lineAt(pos);
        const lineNum = line.number - 1;
        const char = pos - line.from;

        try {
            const hover = await LanguageService.getHover(filePath, lineNum, char);
            if (!hover || !hover.contents) return null;

            return {
                pos,
                // We could calculate exact end from hover.range if provided
                create(view) {
                    const dom = document.createElement("div");
                    dom.className = "cm-tooltip-hover";
                    dom.style.padding = "8px";
                    dom.style.backgroundColor = "var(--bg-secondary, #1e1e1e)";
                    dom.style.border = "1px solid var(--border-neutral, #333)";
                    dom.style.borderRadius = "4px";
                    dom.style.fontSize = "12px";
                    dom.style.fontFamily = "var(--font-mono, monospace)";
                    dom.style.maxWidth = "400px";
                    dom.style.maxHeight = "300px";
                    dom.style.overflow = "auto";
                    dom.style.whiteSpace = "pre-wrap";

                    // Simple content rendering
                    dom.textContent = hover.contents || "";
                    return { dom };
                }
            };
        } catch (e) {
            console.error("Hover failed", e);
            return null;
        }
    });
}

function mapKind(kind: string | null): "class" | "constant" | "enum" | "function" | "interface" | "keyword" | "method" | "namespace" | "property" | "text" | "type" | "variable" | undefined {
    if (!kind) return undefined;
    // Basic mapping from LSP kind integers (stringified)
    // 1=Text, 2=Method, 3=Function, 4=Constructor, 5=Field, 6=Variable, 7=Class, 8=Interface, 9=Module
    // 10=Property, 11=Unit, 12=Value, 13=Enum, 14=Keyword, 15=Snippet, 25=Struct...
    switch (kind) {
        case "1": return "text";
        case "2": return "method";
        case "3": return "function";
        case "7": return "class";
        case "8": return "interface";
        case "9": return "namespace";
        case "10": return "property";
        case "13": return "enum";
        case "14": return "keyword";
        case "6": return "variable";
        default: return undefined;
    }
}

export function languageFeatures(filePath: string): Extension {
    return [
        autocompletion({ override: [createCompletionSource(filePath)] }),
        createHoverTooltip(filePath)
    ];
}
