import type { Change, PatchHunk } from '../types/change';
import { v4 as uuidv4 } from 'uuid';

/**
 * Parsed arguments from an apply_patch tool call
 */
interface ApplyPatchArgs {
    path: string;
    // New multi-patch format
    patches?: PatchHunk[];
    // Legacy single-patch format
    old_text?: string;
    new_text?: string;
}

/**
 * Parses an apply_patch tool call's arguments and converts them to Change objects.
 * Supports both legacy single-patch and new multi-patch formats.
 * 
 * @param toolCallId - The ID of the tool call (used as base for change IDs)
 * @param argumentsJson - The raw JSON arguments string from the tool call
 * @returns Array of Change objects to propose to the user
 */
export function parseApplyPatchToChanges(toolCallId: string, argumentsJson: string): Change[] {
    let args: ApplyPatchArgs;

    try {
        args = JSON.parse(argumentsJson);
    } catch (e) {
        console.error('[parseApplyPatchToChanges] Failed to parse arguments:', e);
        return [];
    }

    if (!args.path) {
        console.error('[parseApplyPatchToChanges] Missing path in arguments');
        return [];
    }

    // Check for new multi-patch format
    if (args.patches && Array.isArray(args.patches) && args.patches.length > 0) {
        // Multi-patch: Create a single multi_patch Change
        return [{
            change_type: 'multi_patch',
            id: toolCallId,
            path: args.path,
            patches: args.patches.map(p => ({
                old_text: p.old_text,
                new_text: p.new_text,
                start_line: p.start_line,
                end_line: p.end_line,
            })),
        }];
    }

    // Legacy single-patch format
    if (args.old_text !== undefined && args.new_text !== undefined) {
        return [{
            change_type: 'patch',
            id: toolCallId,
            path: args.path,
            old_content: args.old_text,
            new_content: args.new_text,
        }];
    }

    console.error('[parseApplyPatchToChanges] No patches or old_text/new_text found in arguments');
    return [];
}

/**
 * Checks if a tool call is an apply_patch that should be converted to changes.
 */
export function isApplyPatchToolCall(toolName: string): boolean {
    return toolName === 'apply_patch';
}

/**
 * Checks if a Change is a multi-patch type.
 */
export function isMultiPatchChange(change: Change): change is Change & { change_type: 'multi_patch' } {
    return change.change_type === 'multi_patch';
}

/**
 * Gets the patch count for display purposes.
 */
export function getPatchCount(change: Change): number {
    if (change.change_type === 'multi_patch') {
        return change.patches.length;
    }
    if (change.change_type === 'patch') {
        return 1;
    }
    return 0;
}
