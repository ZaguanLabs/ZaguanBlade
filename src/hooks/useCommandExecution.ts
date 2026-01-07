import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

export interface CommandExecution {
    commandId: string;
    callId: string;
    command: string;
    cwd?: string;
    output?: string;
    exitCode?: number;
    isRunning: boolean;
}

export function useCommandExecution() {
    const [executions, setExecutions] = useState<Map<string, CommandExecution>>(new Map());

    useEffect(() => {
        let unlistenStart: (() => void) | undefined;

        const setupListeners = async () => {
            // Listen for command execution started events
            unlistenStart = await listen<{
                command_id: string;
                call_id: string;
                command: string;
                cwd?: string;
            }>('command-execution-started', (event) => {
                console.log('[CMD EXEC] Started:', event.payload);
                
                setExecutions(prev => {
                    const next = new Map(prev);
                    next.set(event.payload.call_id, {
                        commandId: event.payload.command_id,
                        callId: event.payload.call_id,
                        command: event.payload.command,
                        cwd: event.payload.cwd,
                        isRunning: true,
                    });
                    return next;
                });
            });
        };

        setupListeners();

        return () => {
            if (unlistenStart) unlistenStart();
        };
    }, []);

    const handleCommandComplete = async (callId: string, output: string, exitCode: number) => {
        console.log('[CMD EXEC] Complete:', { callId, exitCode, outputLength: output.length });
        
        // Update local state
        setExecutions(prev => {
            const next = new Map(prev);
            const exec = next.get(callId);
            if (exec) {
                next.set(callId, {
                    ...exec,
                    output,
                    exitCode,
                    isRunning: false,
                });
            }
            return next;
        });

        // Submit result to backend
        try {
            await invoke('submit_command_result', {
                callId,
                output,
                exitCode,
            });
            console.log('[CMD EXEC] Result submitted to backend');
        } catch (err) {
            console.error('[CMD EXEC] Failed to submit result:', err);
        }
    };

    return {
        executions,
        handleCommandComplete,
    };
}
