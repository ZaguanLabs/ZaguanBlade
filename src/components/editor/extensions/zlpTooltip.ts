import { hoverTooltip } from "@codemirror/view";
import { Extension } from "@codemirror/state";
import { ZLPService } from "../../../services/zlp";

/**
 * Hover tooltip extension that shows ZLP symbol information
 * Displays type info, documentation, and other metadata when hovering over symbols
 */
export function zlpHoverTooltip(filename: string): Extension {
    return hoverTooltip(async (view, pos, side) => {
        if (!filename) return null;

        try {
            // Get the word at the cursor position
            const line = view.state.doc.lineAt(pos);
            const lineText = line.text;
            const col = pos - line.from;

            // Simple word boundary detection
            let start = col;
            let end = col;
            
            // Find word start
            while (start > 0 && /[\w]/.test(lineText[start - 1])) {
                start--;
            }
            
            // Find word end
            while (end < lineText.length && /[\w]/.test(lineText[end])) {
                end++;
            }

            const word = lineText.slice(start, end);
            if (!word || word.length === 0) return null;

            // Get structure to find symbol info
            const structure = await ZLPService.getStructure(filename, "");
            
            // Find the symbol at this position
            const findSymbol = (nodes: any[]): any => {
                for (const node of nodes) {
                    const nodeStartLine = node.range.start.line;
                    const nodeEndLine = node.range.end.line;
                    const currentLine = line.number - 1; // Convert to 0-based

                    if (currentLine >= nodeStartLine && currentLine <= nodeEndLine) {
                        if (node.name === word) {
                            return node;
                        }
                        if (node.children) {
                            const child = findSymbol(node.children);
                            if (child) return child;
                        }
                    }
                }
                return null;
            };

            const symbol = findSymbol(structure);
            if (!symbol) return null;

            // Create tooltip content
            return {
                pos,
                above: true,
                create() {
                    const dom = document.createElement("div");
                    dom.className = "cm-zlp-tooltip";
                    
                    // Symbol kind badge
                    const kindBadge = document.createElement("span");
                    kindBadge.className = "cm-zlp-kind";
                    kindBadge.textContent = symbol.kind;
                    dom.appendChild(kindBadge);
                    
                    // Symbol name
                    const name = document.createElement("div");
                    name.className = "cm-zlp-name";
                    name.textContent = symbol.name;
                    dom.appendChild(name);
                    
                    // Symbol signature (if available)
                    if (symbol.signature) {
                        const sig = document.createElement("div");
                        sig.className = "cm-zlp-signature";
                        sig.textContent = symbol.signature;
                        dom.appendChild(sig);
                    }
                    
                    // Location info
                    const location = document.createElement("div");
                    location.className = "cm-zlp-location";
                    location.textContent = `Line ${symbol.range.start.line + 1}`;
                    dom.appendChild(location);
                    
                    return { dom };
                }
            };
        } catch (error) {
            console.warn('[ZLP Tooltip] Failed to get symbol info:', error);
            return null;
        }
    }, {
        // Hover delay in milliseconds
        hoverTime: 500
    });
}
