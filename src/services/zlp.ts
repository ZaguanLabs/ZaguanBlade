import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { v4 as uuidv4 } from 'uuid';
import { BladeDispatcher } from './blade';
import type { BladeEventEnvelope } from '../types/blade';
import type { ZLPCapabilitiesResult, ZLPStructureResponse, ZLPValidationError, ZLPValidationResponse, ZLPGraphResponse } from '../types/zlp';

export class ZLPService {
    private static TIMEOUT_MS = 15000;

    /**
     * Check server capabilities (The "Ping")
     */
    static async capabilities(): Promise<ZLPCapabilitiesResult> {
        return this.send<ZLPCapabilitiesResult>("zlp.capabilities", {
            client_name: "zblade",
            version: "0.0.4-alpha"
        });
    }

    /**
     * Get structural outline of the file
     */
    static async getStructure(file: string, content: string): Promise<ZLPStructureResponse> {
        return this.send<ZLPStructureResponse>("zlp.structure", { file, content });
    }

    /**
     * Get diagnostics for the file
     */
    static async getDiagnostics(file: string, content: string, language: string): Promise<ZLPValidationError[]> {
        // ZLP validation returns { errors: [], ... }
        const result = await this.send<ZLPValidationResponse>("zlp.validate", {
            path: file,
            content,
            language,
            mode: "fast"
        });
        return result.errors || [];
    }

    /**
     * Get call graph for a symbol
     */
    static async getCallGraph(symbolId: string): Promise<ZLPGraphResponse> {
        return this.send<ZLPGraphResponse>("zlp.graph", { symbol_id: symbolId, direction: "both", depth: 1 });
    }

    /**
     * Send a raw ZLP request and await the response.
     */
    static async send<T = any>(method: string, params: any): Promise<T> {
        const id = uuidv4();
        let unlisten: UnlistenFn | undefined;

        // Create the promise that will resolve when the event arrives
        const responsePromise = new Promise<T>((resolve, reject) => {
            // 1. Setup Timeout
            const timeoutId = setTimeout(() => {
                if (unlisten) unlisten();
                reject(new Error(`ZLP Request '${method}' timed out after ${ZLPService.TIMEOUT_MS}ms`));
            }, ZLPService.TIMEOUT_MS);

            // 2. Setup Listener
            listen<BladeEventEnvelope>('blade-event', (event) => {
                const envelope = event.payload;

                // Filter for Language events
                if (envelope.event.type === 'Language') {
                    const langEvent = envelope.event.payload as any;

                    // Filter for ZlpResponse matching our ID
                    if (langEvent.type === 'ZlpResponse' &&
                        langEvent.payload.original_request_id === id) {

                        clearTimeout(timeoutId);
                        if (unlisten) unlisten();

                        // Resolve with the inner payload (the ZLP result)
                        resolve(langEvent.payload.payload as T);
                    }
                }
            }).then((u) => {
                unlisten = u;
            }).catch((err) => {
                clearTimeout(timeoutId);
                reject(new Error(`Failed to setup listener: ${err}`));
            });
        });

        // 3. Dispatch the Intent
        try {
            await BladeDispatcher.language({
                type: "ZlpMessage",
                payload: { method, params }
            }, id);
        } catch (e) {
            // If dispatch fails, cleanup and throw
            if (unlisten) unlisten();
            throw e;
        }

        return responsePromise;
    }
}
