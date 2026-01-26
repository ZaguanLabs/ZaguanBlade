/**
 * Editor Facade Service
 *
 * Provides a unified interface for editor operations that routes to either
 * frontend-owned state (legacy) or backend-authoritative state (headless)
 * based on feature flags.
 *
 * Part of the Strangler Fig migration pattern for headless architecture.
 */

import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from './blade';
import type { FeatureFlagsSnapshot } from '../types/coreState';

let featureFlags: FeatureFlagsSnapshot | null = null;

/**
 * Initialize the editor facade by loading feature flags from backend.
 * Should be called once at app startup.
 */
export async function initEditorFacade(): Promise<void> {
    try {
        featureFlags = await invoke<FeatureFlagsSnapshot>('get_feature_flags');
        console.log('[EditorFacade] Initialized with flags:', featureFlags);
    } catch (e) {
        console.warn('[EditorFacade] Failed to load feature flags, using defaults:', e);
        featureFlags = {
            editor_backend_authority: false,
            tabs_backend_authority: false,
        };
    }
}

/**
 * Check if backend has authority over editor state.
 */
export function isBackendAuthoritative(): boolean {
    return featureFlags?.editor_backend_authority ?? false;
}

/**
 * Passthrough error - thrown when the facade should not handle the operation
 * and the caller should use its own (legacy) implementation.
 */
export class PassthroughError extends Error {
    constructor() {
        super('PASSTHROUGH');
        this.name = 'PassthroughError';
    }
}

/**
 * Editor Facade - routes operations based on feature flags.
 *
 * When backend authority is enabled:
 * - Operations dispatch intents to Rust
 * - State changes come back via blade-event
 *
 * When backend authority is disabled:
 * - Throws PassthroughError so caller can use legacy behavior
 */
export const EditorFacade = {
    /**
     * Open a file. In backend authority mode, this updates Rust state
     * and emits FileOpened event.
     */
    async openFile(path: string): Promise<void> {
        if (!isBackendAuthoritative()) {
            throw new PassthroughError();
        }

        await BladeDispatcher.editor({
            type: 'OpenFile',
            payload: { path }
        });
    },

    /**
     * Close a file. In backend authority mode, this updates Rust state
     * and emits FileClosed event.
     */
    async closeFile(path: string): Promise<void> {
        if (!isBackendAuthoritative()) {
            throw new PassthroughError();
        }

        await BladeDispatcher.editor({
            type: 'CloseFile',
            payload: { path }
        });
    },

    /**
     * Set the active file. In backend authority mode, this updates Rust state
     * and emits ActiveFileChanged event.
     */
    async setActiveFile(path: string | null): Promise<void> {
        if (!isBackendAuthoritative()) {
            throw new PassthroughError();
        }

        await BladeDispatcher.editor({
            type: 'SetActiveFile',
            payload: { path }
        });
    },

    /**
     * Update cursor position. Always syncs to backend for AI context,
     * regardless of authority mode.
     */
    async updateCursor(line: number, column: number): Promise<void> {
        // Always sync cursor to backend for AI context (fire and forget)
        BladeDispatcher.editor({
            type: 'UpdateCursor',
            payload: { line, column }
        }).catch(() => {
            // Ignore errors - cursor sync is best-effort
        });
    },

    /**
     * Update selection. Always syncs to backend for AI context,
     * regardless of authority mode.
     */
    async updateSelection(start: number, end: number): Promise<void> {
        // Always sync selection to backend for AI context (fire and forget)
        BladeDispatcher.editor({
            type: 'UpdateSelection',
            payload: { start, end }
        }).catch(() => {
            // Ignore errors - selection sync is best-effort
        });
    },

    /**
     * Request current editor state snapshot from backend.
     * Useful for recovery/sync scenarios.
     */
    async getState(): Promise<void> {
        await BladeDispatcher.editor({
            type: 'GetState',
        });
        // State will be returned via blade-event EditorEvent.StateSnapshot
    },
};
