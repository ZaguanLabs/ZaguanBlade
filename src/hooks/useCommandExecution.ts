import { useEffect, useState, useRef, useCallback } from 'react';
import { emit, listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import { BLADE_TERMINAL_ID } from '../constants/terminal';
import type { BladeEventEnvelope, TerminalEvent } from '../types/blade';

export interface CommandExecution {
    commandId: string;
    callId: string;
    command: string;
    cwd?: string;
    output?: string;
    exitCode?: number;
    isRunning: boolean;
}

type PendingCommand = {
    callId: string;
    command: string;
    cwd?: string;
    started: boolean;
};

const SENTINEL_START = '##BLADE_CMD_START:';
const SENTINEL_EXIT = '##BLADE_CMD_EXIT:';
const SENTINEL_END = '##';

export function useCommandExecution() {
    const [executions, setExecutions] = useState<Map<string, CommandExecution>>(new Map());
    const pendingCommandsRef = useRef<Map<string, PendingCommand>>(new Map());
    const bladeReadyRef = useRef(false);
    const pendingInputQueueRef = useRef<(() => void)[]>([]);

    const handleCommandComplete = useCallback(async (callId: string, output: string, exitCode: number) => {
        console.log('[CMD EXEC] Complete:', { callId, exitCode, outputLength: output.length });

        // Update local state - Remove execution to unmount terminal
        setExecutions(prev => {
            const next = new Map(prev);
            next.delete(callId);
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
    }, []);

    const flushPendingInputs = useCallback(() => {
        const queue = pendingInputQueueRef.current;
        pendingInputQueueRef.current = [];
        queue.forEach(fn => fn());
    }, []);

    const enqueueBladeInput = useCallback((fn: () => void) => {
        if (bladeReadyRef.current) {
            fn();
            return;
        }
        pendingInputQueueRef.current.push(fn);
    }, []);

    const escapeShellArg = useCallback((value: string) => {
        if (value.length === 0) return "''";
        return `'${value.replace(/'/g, `'"'"'`)}'`;
    }, []);

    const sendCommandToBlade = useCallback((pending: PendingCommand) => {
        const { callId, command, cwd } = pending;

        const parts: string[] = [];
        parts.push(`echo '${SENTINEL_START}${callId}${SENTINEL_END}'`);
        if (cwd) {
            parts.push(`cd ${escapeShellArg(cwd)}`);
        }
        parts.push(command);
        parts.push(`__blade_ec=$?; echo '${SENTINEL_EXIT}${callId}:'"$__blade_ec"'${SENTINEL_END}'; exit $__blade_ec`);

        const payload = `( ${parts.join('; ')} )\n`;

        enqueueBladeInput(() => {
            BladeDispatcher.terminal({
                type: 'Input',
                payload: { id: BLADE_TERMINAL_ID, data: payload },
            }).catch(async err => {
                pendingCommandsRef.current.delete(callId);
                await handleCommandComplete(callId, `Failed to send command to Blade terminal: ${String(err)}`, 1);
            });
        });
    }, [enqueueBladeInput, escapeShellArg, handleCommandComplete]);

    useEffect(() => {
        let unlistenStart: (() => void) | undefined;
        let unlistenExit: (() => void) | undefined;
        let unlistenBlade: (() => void) | undefined;
        let unlistenCmdDetected: (() => void) | undefined;
        let unlistenCmdExitDetected: (() => void) | undefined;

        const setupListeners = async () => {
            unlistenBlade = await listen<BladeEventEnvelope>('blade-event', (event) => {
                const bladeEvent = event.payload.event;
                if (bladeEvent.type !== 'Terminal') return;
                const terminalEvent = bladeEvent.payload as TerminalEvent;
                if (terminalEvent.type === 'Spawned' && terminalEvent.payload.id === BLADE_TERMINAL_ID) {
                    bladeReadyRef.current = true;
                    flushPendingInputs();
                }
            });

            unlistenStart = await listen<{
                command_id: string;
                call_id: string;
                command: string;
                cwd?: string;
            }>('command-execution-started', (event) => {
                console.log('[CMD EXEC] Started:', event.payload);

                const pending: PendingCommand = {
                    callId: event.payload.call_id,
                    command: event.payload.command,
                    cwd: event.payload.cwd,
                    started: false,
                };
                pendingCommandsRef.current.set(event.payload.call_id, pending);

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

                emit('open-blade-terminal', { cwd: event.payload.cwd, focus: true })
                    .catch((err: unknown) => console.error('[CMD EXEC] Failed to open Blade terminal:', err));

                pending.started = true;
                sendCommandToBlade(pending);
            });

            unlistenCmdDetected = await listen<{ terminal_id: string; call_id: string }>('blade-cmd-started', () => {
                // No-op: command-execution-started already marked command active and sent input.
            });

            unlistenCmdExitDetected = await listen<{ terminal_id: string; call_id: string; exit_code: number; output: string }>('blade-cmd-exited', (event) => {
                if (event.payload.terminal_id !== BLADE_TERMINAL_ID) return;
                const { call_id: callId, exit_code: exitCode, output } = event.payload;
                if (!pendingCommandsRef.current.has(callId)) return;
                pendingCommandsRef.current.delete(callId);
                handleCommandComplete(callId, output, exitCode);
            });

            unlistenExit = await listen<{ id: string; exit_code: number }>('terminal-exit', (event) => {
                if (event.payload.id === BLADE_TERMINAL_ID) {
                    bladeReadyRef.current = false;
                }
            });
        };

        setupListeners();

        return () => {
            if (unlistenStart) unlistenStart();
            if (unlistenExit) unlistenExit();
            if (unlistenBlade) unlistenBlade();
            if (unlistenCmdDetected) unlistenCmdDetected();
            if (unlistenCmdExitDetected) unlistenCmdExitDetected();
        };
    }, [flushPendingInputs, handleCommandComplete, sendCommandToBlade]);

    return {
        executions,
        handleCommandComplete,
    };
}
