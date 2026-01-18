import { invoke } from '@tauri-apps/api/core';
import { v4 as uuidv4 } from 'uuid';
import type {
    BladeIntent,
    BladeEnvelope,
    BladeIntentEnvelope,
    BladeError,
    ChatIntent,
    EditorIntent,
    FileIntent,
    WorkflowIntent,
    TerminalIntent,
    HistoryIntent,
    LanguageIntent,
    Version
} from '../types/blade';

/**
 * Service for dispatching Blade Protocol intents to the backend.
 * Handles envelope wrapping, UUID generation, and error unwrapping.
 */
export class BladeDispatcher {
    // v1.1: Semantic versioning
    private static readonly PROTOCOL_VERSION: Version = { major: 1, minor: 1, patch: 0 };
    private static readonly PROTOCOL_NAME = "BCP";

    /**
     * Dispatches an Intent to the Backend.
     * @param domain - The domain of the intent (e.g., "Chat", "Editor")
     * @param intent - The intent payload
     * @param idempotencyKey - Optional idempotency key for critical operations
     * @returns Promise that resolves when the intent is ACCEPTED (not necessarily completed)
     */
    static async dispatch(domain: string, intent: BladeIntent, idempotencyKey?: string): Promise<void> {
        const envelope: BladeEnvelope<BladeIntentEnvelope> = {
            protocol: this.PROTOCOL_NAME,
            version: this.PROTOCOL_VERSION,
            domain,
            message: {
                id: uuidv4(),
                timestamp: Date.now(),
                idempotency_key: idempotencyKey, // v1.1: Optional idempotency key
                intent
            }
        };

        try {
            console.log(`[BladeDispatcher] Dispatching ${domain} intent:`, intent);
            await invoke('dispatch', { envelope });
        } catch (error) {
            // Re-throw typed error if it matches BladeError shape, otherwise wrap it
            console.error('[BladeDispatcher] Failed to dispatch:', error);
            const bladeError = error as BladeError; // Simplified casting for now
            throw bladeError;
        }
    }

    // Helper methods for specific domains for cleaner API usage

    static async chat(intent: ChatIntent) {
        return this.dispatch("Chat", { type: "Chat", payload: intent });
    }

    static async editor(intent: EditorIntent) {
        return this.dispatch("Editor", { type: "Editor", payload: intent });
    }

    static async file(intent: FileIntent) {
        return this.dispatch("File", { type: "File", payload: intent });
    }

    static async workflow(intent: WorkflowIntent) {
        return this.dispatch("Workflow", { type: "Workflow", payload: intent });
    }

    static async terminal(intent: TerminalIntent) {
        return this.dispatch("Terminal", { type: "Terminal", payload: intent });
    }

    static async history(intent: HistoryIntent) {
        return this.dispatch("History", { type: "History", payload: intent });
    }

    static async language(intent: LanguageIntent) {
        return this.dispatch("Language", { type: "Language", payload: intent });
    }
}
