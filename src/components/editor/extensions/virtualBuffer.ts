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
