import { StateField, StateEffect } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

export interface PendingDiff {
    id: string;
    old_content: string;
    new_content: string;
    accepted: boolean; // true = applied to virtual buffer, false = shown in overlay
}

export interface VirtualBufferState {
    baseContent: string;        // Original content from disk
    pendingDiffs: PendingDiff[]; // All diffs (accepted + pending)
    isDirty: boolean;            // Has uncommitted virtual changes
}

// State Effects for virtual buffer operations
export const setBaseContent = StateEffect.define<string>();
export const addPendingDiff = StateEffect.define<PendingDiff>();
export const acceptDiffToVirtual = StateEffect.define<string>(); // diff id
export const rejectDiff = StateEffect.define<string>(); // diff id
export const commitVirtualBuffer = StateEffect.define<void>();
export const discardVirtualChanges = StateEffect.define<void>();

// Compute virtual content by applying accepted diffs to base content
function computeVirtualContent(base: string, diffs: PendingDiff[]): string {
    let content = base;
    
    // Apply all accepted diffs in order
    for (const diff of diffs) {
        if (diff.accepted) {
            content = content.replace(diff.old_content, diff.new_content);
        }
    }
    
    return content;
}

// Virtual Buffer StateField
export const virtualBufferField = StateField.define<VirtualBufferState>({
    create() {
        return {
            baseContent: "",
            pendingDiffs: [],
            isDirty: false
        };
    },
    
    update(value, tr) {
        let newState = value;
        
        for (const effect of tr.effects) {
            if (effect.is(setBaseContent)) {
                // Update base content (e.g., when file is loaded/reloaded)
                newState = {
                    baseContent: effect.value,
                    pendingDiffs: [],
                    isDirty: false
                };
            } else if (effect.is(addPendingDiff)) {
                // Add a new diff (from AI suggestion)
                newState = {
                    ...newState,
                    pendingDiffs: [...newState.pendingDiffs, effect.value]
                };
            } else if (effect.is(acceptDiffToVirtual)) {
                // Mark diff as accepted (apply to virtual buffer)
                newState = {
                    ...newState,
                    pendingDiffs: newState.pendingDiffs.map(d => 
                        d.id === effect.value ? { ...d, accepted: true } : d
                    ),
                    isDirty: true
                };
            } else if (effect.is(rejectDiff)) {
                // Remove diff entirely
                newState = {
                    ...newState,
                    pendingDiffs: newState.pendingDiffs.filter(d => d.id !== effect.value)
                };
            } else if (effect.is(commitVirtualBuffer)) {
                // Committed to disk - update base content and clear diffs
                const virtualContent = computeVirtualContent(newState.baseContent, newState.pendingDiffs);
                newState = {
                    baseContent: virtualContent,
                    pendingDiffs: [],
                    isDirty: false
                };
            } else if (effect.is(discardVirtualChanges)) {
                // Discard all virtual changes
                newState = {
                    ...newState,
                    pendingDiffs: [],
                    isDirty: false
                };
            }
        }
        
        return newState;
    }
});

// Helper to get current virtual content from editor state
export function getVirtualContent(view: EditorView): string {
    const bufferState = view.state.field(virtualBufferField);
    return computeVirtualContent(bufferState.baseContent, bufferState.pendingDiffs);
}

// Helper to check if file has uncommitted virtual changes
export function hasVirtualChanges(view: EditorView): boolean {
    const bufferState = view.state.field(virtualBufferField);
    return bufferState.isDirty;
}

// Helper to get pending (not accepted) diffs for overlay display
export function getPendingDiffsForOverlay(view: EditorView): PendingDiff[] {
    const bufferState = view.state.field(virtualBufferField);
    return bufferState.pendingDiffs.filter(d => !d.accepted);
}
