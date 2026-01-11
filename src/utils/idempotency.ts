/**
 * Idempotency Key Generation for v1.1 Protocol
 * 
 * Generates unique, deterministic keys for critical operations to prevent
 * double-execution on retry.
 */

import { v4 as uuidv4 } from 'uuid';

/**
 * Generate an idempotency key for a specific operation
 * 
 * @param operation - Operation name (e.g., "approve-change", "save-file")
 * @param identifier - Unique identifier for the resource (e.g., change_id, file_path)
 * @param timestamp - Optional timestamp (defaults to current time)
 * @returns Idempotency key string
 */
export function generateIdempotencyKey(
    operation: string,
    identifier: string,
    timestamp?: number
): string {
    const ts = timestamp ?? Date.now();
    return `${operation}-${identifier}-${ts}`;
}

/**
 * Generate a random idempotency key (for operations without deterministic identifiers)
 * 
 * @param operation - Operation name
 * @returns Random idempotency key string
 */
export function generateRandomIdempotencyKey(operation: string): string {
    return `${operation}-${uuidv4()}`;
}

/**
 * Idempotency key cache for tracking recent operations
 * Useful for detecting duplicate requests on the client side
 */
export class IdempotencyKeyCache {
    private cache: Map<string, { key: string; timestamp: number }>;
    private ttl: number;

    constructor(ttlMs: number = 60000) { // Default 1 minute
        this.cache = new Map();
        this.ttl = ttlMs;
    }

    /**
     * Store an idempotency key
     */
    set(operation: string, identifier: string, key: string): void {
        const cacheKey = `${operation}:${identifier}`;
        this.cache.set(cacheKey, { key, timestamp: Date.now() });
        this.cleanup();
    }

    /**
     * Get a cached idempotency key if it exists and hasn't expired
     */
    get(operation: string, identifier: string): string | null {
        const cacheKey = `${operation}:${identifier}`;
        const entry = this.cache.get(cacheKey);
        
        if (!entry) return null;
        
        // Check if expired
        if (Date.now() - entry.timestamp > this.ttl) {
            this.cache.delete(cacheKey);
            return null;
        }
        
        return entry.key;
    }

    /**
     * Check if an operation is already in progress
     */
    has(operation: string, identifier: string): boolean {
        return this.get(operation, identifier) !== null;
    }

    /**
     * Remove expired entries
     */
    private cleanup(): void {
        const now = Date.now();
        const toDelete: string[] = [];
        
        this.cache.forEach((entry, key) => {
            if (now - entry.timestamp > this.ttl) {
                toDelete.push(key);
            }
        });
        
        toDelete.forEach(key => this.cache.delete(key));
    }

    /**
     * Clear all cached keys
     */
    clear(): void {
        this.cache.clear();
    }

    /**
     * Get cache size
     */
    size(): number {
        this.cleanup();
        return this.cache.size;
    }
}

/**
 * Global idempotency key cache instance
 */
export const idempotencyCache = new IdempotencyKeyCache();

/**
 * Helper function to get or create an idempotency key for an operation
 * Uses cache to prevent duplicate keys for the same operation
 */
export function getOrCreateIdempotencyKey(
    operation: string,
    identifier: string
): string {
    // Check cache first
    const cached = idempotencyCache.get(operation, identifier);
    if (cached) {
        console.log(`[Idempotency] Using cached key for ${operation}:${identifier}`);
        return cached;
    }
    
    // Generate new key
    const key = generateIdempotencyKey(operation, identifier);
    idempotencyCache.set(operation, identifier, key);
    
    console.log(`[Idempotency] Generated new key for ${operation}:${identifier}`);
    return key;
}

/**
 * Critical operations that should use idempotency keys
 */
export const IDEMPOTENT_OPERATIONS = {
    APPROVE_CHANGE: 'approve-change',
    APPROVE_ALL: 'approve-all',
    REJECT_CHANGE: 'reject-change',
    REJECT_ALL: 'reject-all',
    SAVE_FILE: 'save-file',
    DELETE_FILE: 'delete-file',
    EXECUTE_COMMAND: 'execute-command',
} as const;

export type IdempotentOperation = typeof IDEMPOTENT_OPERATIONS[keyof typeof IDEMPOTENT_OPERATIONS];
