import { v4 as uuidv4 } from 'uuid';
import i18n from '../i18n';
import { BladeDispatcher } from './blade';
import { subscribeBladeNestedEventType } from './bladeEvents';
import type { ZLPStructureResponse, ZLPValidationError, ZLPValidationResponse, StructureNode } from '../types/zlp';

export class ZLPService {
    private static TIMEOUT_MS = 15000;

    /**
     * Get structural outline of the file
     */
    static async getStructure(file: string, content: string): Promise<ZLPStructureResponse> {
        const response = await this.send<any>("zlp.structure", { file, content });

        const payload = response?.result ?? response?.data ?? response;

        if (Array.isArray(payload)) return payload;
        if (Array.isArray(payload?.nodes)) return payload.nodes;

        if (payload && typeof payload === 'object') {
            const buckets = ['functions', 'imports', 'types', 'variables', 'classes', 'methods', 'symbols'];
            const hasBucket = buckets.some((key) =>
                Object.prototype.hasOwnProperty.call(payload, key)
            );
            if (hasBucket) {
                const flattened = buckets.flatMap((key) =>
                    Array.isArray((payload as Record<string, unknown>)[key])
                        ? ((payload as Record<string, unknown>)[key] as StructureNode[])
                        : []
                );
                return flattened;
            }
        }

        console.warn('[ZLP] Unexpected structure response shape:', payload);
        return [];
    }

    /**
     * Get diagnostics for the file
     */
    static async getDiagnostics(file: string, content: string, language: string): Promise<ZLPValidationError[]> {
        // ZLP validation returns { errors: [], ... }
        const result = await this.send<any>("zlp.validate", {
            path: file,
            content,
            language,
            mode: "fast"
        });
        if (Array.isArray(result)) return result;

        const errors = result?.errors ?? result?.result?.errors ?? [];
        if (!Array.isArray(errors)) {
            console.warn('[ZLP] Unexpected validation response shape:', result);
            return [];
        }

        return errors;
    }

    /**
     * Send a raw ZLP request and await the response.
     */
    static async send<T = any>(method: string, params: any): Promise<T> {
        const id = uuidv4();
        let unsubscribe: (() => void) | undefined;

        // Create the promise that will resolve when the event arrives
        const responsePromise = new Promise<T>((resolve, reject) => {
            // 1. Setup Timeout
            const timeoutId = setTimeout(() => {
                if (unsubscribe) unsubscribe();
                reject(new Error(i18n.t('errors.zlpTimeout', {
                    defaultValue: "ZLP Request '{{method}}' timed out after {{timeout}}ms",
                    method,
                    timeout: ZLPService.TIMEOUT_MS
                })));
            }, ZLPService.TIMEOUT_MS);

            // 2. Setup Listener
            unsubscribe = subscribeBladeNestedEventType('Language', 'ZlpResponse', (payload) => {
                if (payload.original_request_id === id) {
                    clearTimeout(timeoutId);
                    if (unsubscribe) unsubscribe();

                    const zlpResult = payload.result as any;
                    if (zlpResult?.error) {
                        reject(new Error(zlpResult.error.message || i18n.t('errors.zlpError', { defaultValue: 'ZLP error' })));
                        return;
                    }

                    const normalized = zlpResult?.result ?? zlpResult;
                    resolve(normalized as T);
                }
            });
        });

        // 3. Dispatch the Intent
        try {
            await BladeDispatcher.language({
                type: "ZlpMessage",
                payload: { data: { method, params } }
            }, id);
        } catch (e) {
            // If dispatch fails, cleanup and throw
            if (unsubscribe) unsubscribe();
            throw e;
        }

        return responsePromise;
    }
}
