import { StateField, StateEffect } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

// Simplified state - only track base content for reference
export interface VirtualBufferState {
    baseContent: string; // Original content from disk
}

// State effect for updating base content
export const setBaseContent = StateEffect.define<string>();

// Virtual Buffer StateField - simplified to only track base content
export const virtualBufferField = StateField.define<VirtualBufferState>({
    create() {
        return {
            baseContent: ""
        };
    },
    
    update(value, tr) {
        for (const effect of tr.effects) {
            if (effect.is(setBaseContent)) {
                return {
                    baseContent: effect.value
                };
            }
        }
        return value;
    }
});

// Helper to get base content from editor state
export function getVirtualContent(view: EditorView): string {
    const bufferState = view.state.field(virtualBufferField);
    return bufferState.baseContent;
}

// Helper to check if content has changed from base
export function hasVirtualChanges(view: EditorView): boolean {
    const bufferState = view.state.field(virtualBufferField);
    return view.state.doc.toString() !== bufferState.baseContent;
}
