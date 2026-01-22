
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { v4 as uuidv4 } from 'uuid';
import { BladeDispatcher } from './blade';
import {
    BladeEventEnvelope,
    LanguageIntent,
    LanguageSymbol,
    LanguageEvent
} from '../types/blade';



/**
 * Service for interacting with the backend Language Service (Tree-sitter + LSP).
 * Provides methods for indexing, searching, and retrieving LSP features.
 * Handles the async request/response correlation over the Blade Change Protocol.
 */
export class LanguageService {

    /**
     * Index a single file using Tree-sitter.
     * @param filePath - Absolute path to the file
     */
    static async indexFile(filePath: string): Promise<void> {
        // No response needed
        await BladeDispatcher.language({
            type: "IndexFile",
            payload: { file_path: filePath }
        });
    }

    /**
     * Index the entire workspace (recursively).
     */
    static async indexWorkspace(): Promise<void> {
        // No response needed
        await BladeDispatcher.language({
            type: "IndexWorkspace"
        });
    }

    /**
     * Search for symbols in the index.
     */
    static async searchSymbols(query: string, filePath?: string, symbolTypes?: string[]): Promise<LanguageSymbol[]> {
        return this.request<LanguageSymbol[]>("SearchSymbols", {
            type: "SearchSymbols",
            payload: { query, file_path: filePath, symbol_types: symbolTypes }
        }, (event) => {
            if (event.type === 'SymbolsFound') return event.payload.symbols;
            return undefined;
        });
    }



    /**
     * Notify the backend that a file was opened.
     * This enables LSP servers to track the file content.
     */
    static async didOpen(filePath: string, content: string, languageId: string): Promise<void> {
        await BladeDispatcher.language({
            type: "DidOpen",
            payload: { file_path: filePath, content, language_id: languageId }
        });
    }

    /**
     * Notify the backend that a file's content changed.
     * Should be called on each edit (debounced for performance).
     */
    static async didChange(filePath: string, content: string, version: number): Promise<void> {
        await BladeDispatcher.language({
            type: "DidChange",
            payload: { file_path: filePath, content, version }
        });
    }

    /**
     * Notify the backend that a file was closed.
     */
    static async didClose(filePath: string): Promise<void> {
        await BladeDispatcher.language({
            type: "DidClose",
            payload: { file_path: filePath }
        });
    }



    /**
     * Helper to correlate a request intent with its corresponding response event.
     * Starts listening for the event BEFORE dispatching the intent to ensuring no race conditions.
     */
    private static async request<T>(
        operationName: string,
        intent: LanguageIntent,
        extractor: (event: LanguageEvent) => T | undefined,
        timeoutMs: number = 5000
    ): Promise<T> {
        let unlisten: UnlistenFn | undefined;
        const intentId = uuidv4();

        try {
            // 1. Setup Promise which resolves when event arrives
            const responsePromise = new Promise<T>(async (resolve, reject) => {
                const timeout = setTimeout(() => {
                    reject(new Error(`Timeout waiting for ${operationName} response (ID: ${intentId})`));
                }, timeoutMs);

                unlisten = await listen<BladeEventEnvelope>('blade-event', (event) => {
                    const envelope = event.payload;
                    // Check correlation
                    // We match on causality_id (which should be the intent_id)
                    if (envelope.causality_id === intentId && envelope.event.type === 'Language') {
                        const languageEvent = envelope.event.payload;
                        const result = extractor(languageEvent);
                        if (result !== undefined) {
                            clearTimeout(timeout);
                            resolve(result);
                        }
                    }
                });
            });

            // 2. Dispatch intent with explicit ID
            await BladeDispatcher.language(intent, intentId);

            // 3. Wait for response
            return await responsePromise;

        } finally {
            if (unlisten) unlisten();
        }
    }
}
