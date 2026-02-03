import { useEffect, useState, useRef, useCallback } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
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
    commandId: string;
    callId: string;
    command: string;
    cwd?: string;
    output: string;
    started: boolean;
};

const OSC_PREFIX_RAW = '\x1b]633;';
const OSC_TERMINATOR_RAW = '\x07';
const OSC_PREFIX_LITERAL = '\\x1b]633;';
const OSC_TERMINATOR_LITERAL = '\\x07';
const CMD_START_PREFIX = 'BLADE_CMD_START=';
const CMD_EXIT_PREFIX = 'BLADE_CMD_EXIT=';

export function useCommandExecution() {
    const [executions, setExecutions] = useState<Map<string, CommandExecution>>(new Map());
    const pendingCommandsRef = useRef<Map<string, PendingCommand>>(new Map());
    const activeCallIdRef = useRef<string | null>(null);
    const oscBufferRef = useRef<string>('');
    const bladeReadyRef = useRef<boolean>(false);
    const pendingInputsRef = useRef<Array<() => void>>([]);

    const escapeShellArg = useCallback((value: string) => {
        return `'${value.replace(/'/g, `'\\''`)}'`;
    }, []);

    const enqueueBladeInput = useCallback((send: () => void) => {
        if (bladeReadyRef.current) {
            send();
            return;
        }
        pendingInputsRef.current.push(send);
    }, []);

    const flushPendingInputs = useCallback(() => {
        if (!bladeReadyRef.current) return;
        const queue = pendingInputsRef.current;
        pendingInputsRef.current = [];
        queue.forEach(send => send());
    }, []);

    const sanitizeOutput = useCallback((value: string) => {
        return value
            .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '')
            .replace(/\x1b\[[0-9;?]*[A-Za-z]/g, match => (match.endsWith('m') ? match : ''))
            .replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/g, '');
    }, []);

    const sendCommandToBlade = useCallback((pending: PendingCommand) => {
        const { callId, command, cwd } = pending;
        const parts: string[] = [];
        parts.push(`printf '${OSC_PREFIX_LITERAL}${CMD_START_PREFIX}${callId}${OSC_TERMINATOR_LITERAL}'`);
        if (cwd) {
            parts.push(`cd ${escapeShellArg(cwd)}`);
        }
        parts.push(command);
        parts.push('exit_code=$?');
        parts.push(`printf '${OSC_PREFIX_LITERAL}${CMD_EXIT_PREFIX}${callId};%s${OSC_TERMINATOR_LITERAL}' "$exit_code"`);
        const payload = `${parts.join('; ')}\n`;

        enqueueBladeInput(() => {
            BladeDispatcher.terminal({
                type: 'Input',
                payload: { id: BLADE_TERMINAL_ID, data: payload }
            }).catch(err => console.error('[CMD EXEC] Failed to send command to Blade terminal:', err));
        });
    }, [enqueueBladeInput, escapeShellArg]);

    useEffect(() => {
        let unlistenStart: (() => void) | undefined;
        let unlistenOutput: (() => void) | undefined;
        let unlistenExit: (() => void) | undefined;
        let unlistenBlade: (() => void) | undefined;

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
                    commandId: event.payload.command_id,
                    callId: event.payload.call_id,
                    command: event.payload.command,
                    cwd: event.payload.cwd,
                    output: '',
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
                    .catch(err => console.error('[CMD EXEC] Failed to open Blade terminal:', err));

                sendCommandToBlade(pending);
            });

            unlistenOutput = await listen<{ id: string; data: string }>('terminal-output', (event) => {
                if (event.payload.id !== BLADE_TERMINAL_ID) return;
                const parsed = parseBladeOutput(event.payload.data);
                if (!parsed) return;

                const { cleaned, startedIds, exited } = parsed;
                startedIds.forEach(callId => {
                    const pending = pendingCommandsRef.current.get(callId);
                    if (pending) {
                        pending.started = true;
                        activeCallIdRef.current = callId;
                    }
                });

                if (cleaned) {
                    const activeCallId = activeCallIdRef.current;
                    if (activeCallId) {
                        const pending = pendingCommandsRef.current.get(activeCallId);
                        if (pending) {
                            pending.output += sanitizeOutput(cleaned);
                        }
                    }
                }

                exited.forEach(({ callId, exitCode }) => {
                    const pending = pendingCommandsRef.current.get(callId);
                    if (pending) {
                        handleCommandComplete(callId, pending.output, exitCode);
                        pendingCommandsRef.current.delete(callId);
                        if (activeCallIdRef.current === callId) {
                            activeCallIdRef.current = null;
                        }
                    }
                });
            });

            unlistenExit = await listen<{ id: string; exit_code: number }>('terminal-exit', (event) => {
                if (event.payload.id === BLADE_TERMINAL_ID) {
                    bladeReadyRef.current = false;
                }
            });
        };

        const parseBladeOutput = (data: string) => {
            const combined = `${oscBufferRef.current}${data}`;
            oscBufferRef.current = '';
            let cursor = 0;
            let cleaned = '';
            const startedIds: string[] = [];
            const exited: Array<{ callId: string; exitCode: number }> = [];

            while (cursor < combined.length) {
                const startIdx = combined.indexOf(OSC_PREFIX_RAW, cursor);
                if (startIdx === -1) {
                    cleaned += combined.slice(cursor);
                    break;
                }

                cleaned += combined.slice(cursor, startIdx);
                const endIdx = combined.indexOf(OSC_TERMINATOR_RAW, startIdx);
                if (endIdx === -1) {
                    oscBufferRef.current = combined.slice(startIdx);
                    break;
                }

                const payload = combined.slice(startIdx + OSC_PREFIX_RAW.length, endIdx);
                if (payload.startsWith(CMD_START_PREFIX)) {
                    startedIds.push(payload.slice(CMD_START_PREFIX.length));
                } else if (payload.startsWith(CMD_EXIT_PREFIX)) {
                    const exitPayload = payload.slice(CMD_EXIT_PREFIX.length);
                    const [callId, exitCodeRaw] = exitPayload.split(';');
                    const exitCode = Number(exitCodeRaw);
                    if (callId) {
                        exited.push({ callId, exitCode: Number.isNaN(exitCode) ? 1 : exitCode });
                    }
                }

                cursor = endIdx + OSC_TERMINATOR_RAW.length;
            }

            return { cleaned, startedIds, exited };
        };

        setupListeners();

        return () => {
            if (unlistenStart) unlistenStart();
            if (unlistenOutput) unlistenOutput();
            if (unlistenExit) unlistenExit();
            if (unlistenBlade) unlistenBlade();
        };
    }, [flushPendingInputs, sendCommandToBlade]);

    const handleCommandComplete = async (callId: string, output: string, exitCode: number) => {
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
    };

    return {
        executions,
        handleCommandComplete,
    };
}
