import React, { createContext, startTransition, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { BootstrapState } from '../types/bootstrap';

interface StartupBootstrapContextValue {
    bootstrap: BootstrapState | null;
    isLoading: boolean;
    error: string | null;
    refresh: () => Promise<BootstrapState | null>;
}

const StartupBootstrapContext = createContext<StartupBootstrapContextValue | null>(null);

export const StartupBootstrapProvider: React.FC<React.PropsWithChildren> = ({ children }) => {
    const [bootstrap, setBootstrap] = useState<BootstrapState | null>(null);
    const [isLoading, setIsLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const refresh = useCallback(async () => {
        setIsLoading(true);
        setError(null);

        try {
            const nextBootstrap = await invoke<BootstrapState>('bootstrap_state');
            startTransition(() => {
                setBootstrap(nextBootstrap);
            });
            return nextBootstrap;
        } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            setError(message);
            return null;
        } finally {
            setIsLoading(false);
        }
    }, []);

    useEffect(() => {
        void refresh();
    }, [refresh]);

    const value = useMemo<StartupBootstrapContextValue>(() => ({
        bootstrap,
        isLoading,
        error,
        refresh,
    }), [bootstrap, error, isLoading, refresh]);

    return (
        <StartupBootstrapContext.Provider value={value}>
            {children}
        </StartupBootstrapContext.Provider>
    );
};

export function useStartupBootstrap() {
    const context = useContext(StartupBootstrapContext);
    if (!context) {
        throw new Error('useStartupBootstrap must be used within a StartupBootstrapProvider');
    }
    return context;
}

export function useOptionalStartupBootstrap() {
    return useContext(StartupBootstrapContext);
}
