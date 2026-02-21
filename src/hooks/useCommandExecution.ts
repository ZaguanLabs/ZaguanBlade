import { useEffect, useState, useRef, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';

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
    terminalId: string;
    command: string;
    cwd?: string;
    blocking: boolean;
    waitMsBeforeAsync?: number;
    started: boolean;
};

const SENTINEL_START = '##BLADE_CMD_START:';
const SENTINEL_EXIT = '##BLADE_CMD_EXIT:';
const SENTINEL_END = '##';

export function useCommandExecution() {
    const [executions, setExecutions] = useState<Map<string, CommandExecution>>(new Map());
    const pendingCommandsRef = useRef<Map<string, PendingCommand>>(new Map());

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

    const escapeShellArg = useCallback((value: string) => {
        if (value.length === 0) return "''";
        return `'${value.replace(/'/g, `'"'"'`)}'`;
    }, []);

    const sendCommandToBlade = useCallback((pending: PendingCommand) => {
        const { callId, terminalId, command, cwd, blocking, waitMsBeforeAsync } = pending;

        const parts: string[] = [];
        parts.push(`echo '${SENTINEL_START}${callId}${SENTINEL_END}'`);
        if (cwd) {
            parts.push(`cd ${escapeShellArg(cwd)}`);
        }
        if (blocking) {
            parts.push(command);
        } else {
            const waitMs = typeof waitMsBeforeAsync === 'number' ? Math.max(0, waitMsBeforeAsync) : 1000;
            const waitSeconds = (waitMs / 1000).toString();
            parts.push(`( ${command} ) & __blade_pid=$!`);
            if (waitMs > 0) {
                parts.push(`sleep ${waitSeconds}`);
            }
            parts.push(
                `if kill -0 "$__blade_pid" 2>/dev/null; then disown "$__blade_pid" 2>/dev/null || true; echo '[run_command] detached pid='"$__blade_pid"; __blade_ec=0; else wait "$__blade_pid"; __blade_ec=$?; fi`
            );
        }
        parts.push(`__blade_ec=$?; echo '${SENTINEL_EXIT}${callId}:'"$__blade_ec"'${SENTINEL_END}'; exit $__blade_ec`);

        const payload = `( ${parts.join('; ')} )`;

        BladeDispatcher.terminal({
            type: 'Spawn',
            payload: {
                id: terminalId,
                command: payload,
                interactive: true,
            },
        }).catch(async err => {
            pendingCommandsRef.current.delete(callId);
            await handleCommandComplete(callId, `Failed to start command terminal: ${String(err)}`, 1);
        });
    }, [escapeShellArg, handleCommandComplete]);

    useEffect(() => {
        let unlistenStart: (() => void) | undefined;
        let unlistenExit: (() => void) | undefined;
        let unlistenCmdDetected: (() => void) | undefined;
        let unlistenCmdExitDetected: (() => void) | undefined;

        const setupListeners = async () => {
            unlistenStart = await listen<{
                command_id: string;
                call_id: string;
                command: string;
                cwd?: string;
                blocking?: boolean;
                wait_ms_before_async?: number;
            }>('command-execution-started', (event) => {
                console.log('[CMD EXEC] Started:', event.payload);

                const terminalId = `ai-cmd-${event.payload.call_id}`;
                const pending: PendingCommand = {
                    callId: event.payload.call_id,
                    terminalId,
                    command: event.payload.command,
                    cwd: event.payload.cwd,
                    blocking: event.payload.blocking ?? true,
                    waitMsBeforeAsync: event.payload.wait_ms_before_async,
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

                pending.started = true;
                sendCommandToBlade(pending);
            });

            unlistenCmdDetected = await listen<{ terminal_id: string; call_id: string }>('blade-cmd-started', () => {
                // No-op: command-execution-started already marked command active and sent input.
            });

            unlistenCmdExitDetected = await listen<{ terminal_id: string; call_id: string; exit_code: number; output: string }>('blade-cmd-exited', (event) => {
                const { call_id: callId, exit_code: exitCode, output } = event.payload;
                if (!pendingCommandsRef.current.has(callId)) return;
                pendingCommandsRef.current.delete(callId);
                handleCommandComplete(callId, output, exitCode);
            });

            unlistenExit = await listen<{ id: string; exit_code: number }>('terminal-exit', (event) => {
                const terminalId = event.payload.id;
                const pending = Array.from(pendingCommandsRef.current.values()).find(
                    cmd => cmd.terminalId === terminalId,
                );
                if (pending) {
                    pendingCommandsRef.current.delete(pending.callId);
                    handleCommandComplete(
                        pending.callId,
                        `Command terminal exited before sentinel completion (exit ${event.payload.exit_code}).`,
                        event.payload.exit_code,
                    );
                }
            });
        };

        setupListeners();

        return () => {
            if (unlistenStart) unlistenStart();
            if (unlistenExit) unlistenExit();
            if (unlistenCmdDetected) unlistenCmdDetected();
            if (unlistenCmdExitDetected) unlistenCmdExitDetected();
        };
    }, [handleCommandComplete, sendCommandToBlade]);

    return {
        executions,
        handleCommandComplete,
    };
}
