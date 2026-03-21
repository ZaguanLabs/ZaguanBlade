import { useEffect, useState, useRef, useCallback } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import type { BladeEventEnvelope } from '../types/blade';

import { BLADE_TERMINAL_ID, BLADE_TERMINAL_TITLE } from '../constants/terminal';

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
    terminalTitle: string;
    command: string;
    program?: string;
    args?: string[];
    native: boolean;
    shell?: boolean;
    cwd?: string;
    blocking: boolean;
    waitMsBeforeAsync?: number;
    started: boolean;
    fallbackAttempted: boolean;
};

type ManagedTerminal = {
    id: string;
    title: string;
    cwd?: string;
    ready: boolean;
    opening: boolean;
    activeCallId: string | null;
    backgroundLocked: boolean;
    primary: boolean;
};

const SENTINEL_START = '##BLADE_CMD_START:';
const SENTINEL_EXIT = '##BLADE_CMD_EXIT:';
const SENTINEL_END = '##';
const DETACHED_MARKER = '[run_command] detached pid=';
const TERMINAL_READY_TIMEOUT_MS = 15000;

const buildCommandTerminalId = (callId: string) => `ai-cmd-${callId}`;

export function useCommandExecution() {
    const [executions, setExecutions] = useState<Map<string, CommandExecution>>(new Map());
    const pendingCommandsRef = useRef<Map<string, PendingCommand>>(new Map());
    const terminalsRef = useRef<Map<string, ManagedTerminal>>(new Map([
        [BLADE_TERMINAL_ID, {
            id: BLADE_TERMINAL_ID,
            title: BLADE_TERMINAL_TITLE,
            ready: false,
            opening: false,
            activeCallId: null,
            backgroundLocked: false,
            primary: true,
        }],
    ]));
    const terminalReadyWaitersRef = useRef<Map<string, Array<() => void>>>(new Map());
    const extraTerminalCounterRef = useRef(2);

    const handleCommandComplete = useCallback(async (callId: string, output: string, exitCode: number) => {
        console.debug('[CMD EXEC] Complete:', { callId, exitCode, outputLength: output.length });

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
            console.debug('[CMD EXEC] Result submitted to backend');
        } catch (err) {
            console.error('[CMD EXEC] Failed to submit result:', err);
        }
    }, []);

    const escapeShellArg = useCallback((value: string) => {
        if (value.length === 0) return "''";
        return `'${value.replace(/'/g, `'"'"'`)}'`;
    }, []);

    const buildCommandForExecution = useCallback((pending: PendingCommand) => {
        if (pending.shell === false && pending.program) {
            const escapedProgram = escapeShellArg(pending.program);
            const escapedArgs = (pending.args || []).map(arg => escapeShellArg(arg));
            return [escapedProgram, ...escapedArgs].join(' ');
        }

        return pending.command;
    }, [escapeShellArg]);

    const updateTerminal = useCallback((terminalId: string, updater: (terminal: ManagedTerminal | undefined) => ManagedTerminal | undefined) => {
        const current = terminalsRef.current.get(terminalId);
        const next = updater(current);

        if (next) {
            terminalsRef.current.set(terminalId, next);
            return next;
        }

        terminalsRef.current.delete(terminalId);
        return undefined;
    }, []);

    const resolveTerminalReady = useCallback((terminalId: string) => {
        const waiters = terminalReadyWaitersRef.current.get(terminalId);
        if (!waiters || waiters.length === 0) {
            return;
        }
        terminalReadyWaitersRef.current.delete(terminalId);
        for (const waiter of waiters) {
            waiter();
        }
    }, []);

    const waitForTerminalReady = useCallback((terminalId: string) => {
        const existing = terminalsRef.current.get(terminalId);
        if (existing?.ready) {
            return Promise.resolve();
        }

        return new Promise<void>((resolve, reject) => {
            const timeoutId = window.setTimeout(() => {
                const waiters = terminalReadyWaitersRef.current.get(terminalId) || [];
                terminalReadyWaitersRef.current.set(
                    terminalId,
                    waiters.filter(waiter => waiter !== onReady),
                );
                reject(new Error(`Timed out waiting for terminal ${terminalId} to be ready.`));
            }, TERMINAL_READY_TIMEOUT_MS);

            const onReady = () => {
                window.clearTimeout(timeoutId);
                resolve();
            };

            const waiters = terminalReadyWaitersRef.current.get(terminalId) || [];
            waiters.push(onReady);
            terminalReadyWaitersRef.current.set(terminalId, waiters);
        });
    }, []);

    const openManagedTerminal = useCallback(async (
        terminalId: string,
        title: string,
        cwd?: string,
        command?: string,
        interactive = true,
    ) => {
        const existing = terminalsRef.current.get(terminalId);
        if (existing?.ready) {
            return;
        }

        if (!existing?.opening) {
            updateTerminal(terminalId, terminal => ({
                id: terminalId,
                title: terminal?.title ?? title,
                cwd: cwd ?? terminal?.cwd,
                ready: false,
                opening: true,
                activeCallId: terminal?.activeCallId ?? null,
                backgroundLocked: terminal?.backgroundLocked ?? false,
                primary: terminal?.primary ?? terminalId === BLADE_TERMINAL_ID,
            }));

            await emit('open-terminal', {
                id: terminalId,
                cwd,
                title,
                command,
                interactive,
                focus: true,
                transient: terminalId !== BLADE_TERMINAL_ID,
            });
        }

        await waitForTerminalReady(terminalId);
    }, [updateTerminal, waitForTerminalReady]);

    const createCommandTerminal = useCallback((callId: string, cwd?: string): ManagedTerminal => {
        const terminal: ManagedTerminal = {
            id: buildCommandTerminalId(callId),
            title: `Blade ${extraTerminalCounterRef.current}`,
            cwd,
            ready: false,
            opening: false,
            activeCallId: callId,
            backgroundLocked: false,
            primary: false,
        };
        extraTerminalCounterRef.current += 1;
        terminalsRef.current.set(terminal.id, terminal);
        return terminal;
    }, []);

    const reserveTerminalForCommand = useCallback((callId: string, cwd?: string) => {
        const terminal = createCommandTerminal(callId, cwd);

        updateTerminal(terminal.id, current => {
            const base = current ?? terminal;
            return {
                ...base,
                cwd: cwd ?? base.cwd,
                activeCallId: callId,
            };
        });

        return terminalsRef.current.get(terminal.id) ?? terminal;
    }, [createCommandTerminal, updateTerminal]);

    const buildInteractiveCommandPayload = useCallback((pending: PendingCommand) => {
        const { callId, cwd, blocking, waitMsBeforeAsync } = pending;
        const commandToRun = buildCommandForExecution(pending);

        const parts: string[] = [];
        parts.push(`echo '${SENTINEL_START}${callId}${SENTINEL_END}'`);
        parts.push('__blade_ec=0');

        if (cwd) {
            parts.push(`cd ${escapeShellArg(cwd)} || __blade_ec=$?`);
        }

        if (blocking) {
            parts.push(`if [ "$__blade_ec" -eq 0 ]; then ${commandToRun}; __blade_ec=$?; fi`);
        } else {
            const waitMs = typeof waitMsBeforeAsync === 'number' ? Math.max(0, waitMsBeforeAsync) : 1000;
            const waitSeconds = (waitMs / 1000).toString();
            parts.push(
                `if [ "$__blade_ec" -eq 0 ]; then ( ${commandToRun} ) & __blade_pid=$!; ${waitMs > 0 ? `sleep ${waitSeconds}; ` : ''}if kill -0 "$__blade_pid" 2>/dev/null; then disown "$__blade_pid" 2>/dev/null || true; echo '[run_command] detached pid='"$__blade_pid"; __blade_ec=0; else wait "$__blade_pid"; __blade_ec=$?; fi; fi`
            );
        }

        parts.push(`echo '${SENTINEL_EXIT}${callId}:'"$__blade_ec"'${SENTINEL_END}'`);
        parts.push('unset __blade_ec __blade_pid');

        return `( ${parts.join('; ')} )\n`;
    }, [buildCommandForExecution, escapeShellArg]);

    const releaseTerminalReservation = useCallback((pending: PendingCommand, backgroundLocked: boolean) => {
        updateTerminal(pending.terminalId, terminal => {
            if (!terminal) {
                return terminal;
            }

            return {
                ...terminal,
                opening: false,
                activeCallId: terminal.activeCallId === pending.callId ? null : terminal.activeCallId,
                backgroundLocked,
            };
        });
    }, [updateTerminal]);

    const invalidateTerminal = useCallback((terminalId: string) => {
        updateTerminal(terminalId, terminal => {
            if (!terminal) {
                return terminal;
            }

            if (terminal.primary) {
                return {
                    ...terminal,
                    ready: false,
                    opening: false,
                    activeCallId: null,
                    backgroundLocked: false,
                };
            }

            return undefined;
        });
    }, [updateTerminal]);

    const executeNativeCommand = useCallback(async (pending: PendingCommand) => {
        if (!pending.program) {
            pendingCommandsRef.current.delete(pending.callId);
            await handleCommandComplete(pending.callId, 'Native command execution requires a program.', 1);
            return;
        }

        try {
            await invoke('execute_native_command', {
                callId: pending.callId,
                program: pending.program,
                args: pending.args || [],
                cwd: pending.cwd,
            });
        } catch (err) {
            pendingCommandsRef.current.delete(pending.callId);
            await handleCommandComplete(pending.callId, `Failed to execute native command: ${String(err)}`, 1);
        }
    }, [handleCommandComplete]);

    const sendCommandToBlade = useCallback(async (pending: PendingCommand) => {
        const payload = buildInteractiveCommandPayload(pending);

        try {
            await openManagedTerminal(pending.terminalId, pending.terminalTitle, pending.cwd);
            updateTerminal(pending.terminalId, terminal => terminal ? { ...terminal, ready: true, opening: false } : terminal);
            await BladeDispatcher.terminal({
                type: 'Input',
                payload: {
                    id: pending.terminalId,
                    data: payload,
                },
            });
        } catch (err) {
            invalidateTerminal(pending.terminalId);
            releaseTerminalReservation(pending, false);

            if (!pending.fallbackAttempted) {
                pending.fallbackAttempted = true;
                const fallback = createCommandTerminal(`${pending.callId}-fallback`, pending.cwd);
                pending.terminalId = fallback.id;
                pending.terminalTitle = fallback.title;
                pendingCommandsRef.current.set(pending.callId, pending);
                await sendCommandToBlade(pending);
                return;
            }

            pendingCommandsRef.current.delete(pending.callId);
            await handleCommandComplete(pending.callId, `Failed to execute command in Blade terminal: ${String(err)}`, 1);
        }
    }, [buildInteractiveCommandPayload, createCommandTerminal, handleCommandComplete, invalidateTerminal, openManagedTerminal, releaseTerminalReservation, updateTerminal]);

    const stopCommandExecution = useCallback(async (callId: string) => {
        const pending = pendingCommandsRef.current.get(callId);
        if (!pending) return;

        pendingCommandsRef.current.delete(callId);

        try {
            if (pending.native) {
                await invoke<boolean>('cancel_command_execution', {
                    callId,
                });
            } else {
                await BladeDispatcher.terminal({
                    type: 'Input',
                    payload: {
                        id: pending.terminalId,
                        data: '\u0003',
                    },
                });
            }
        } catch (err) {
            console.error('[CMD EXEC] Failed to interrupt terminal:', err);
        }

        if (!pending.native) {
            releaseTerminalReservation(pending, false);
            await handleCommandComplete(callId, 'Command cancelled by user.', 130);
        }
    }, [handleCommandComplete, releaseTerminalReservation]);

    useEffect(() => {
        let unlistenStart: (() => void) | undefined;
        let unlistenTerminalReady: (() => void) | undefined;
        let unlistenExit: (() => void) | undefined;
        let unlistenCmdDetected: (() => void) | undefined;
        let unlistenCmdExitDetected: (() => void) | undefined;

        const setupListeners = async () => {
            unlistenStart = await listen<{
                command_id: string;
                call_id: string;
                command: string;
                program?: string;
                args?: string[];
                shell?: boolean;
                cwd?: string;
                blocking?: boolean;
                wait_ms_before_async?: number;
            }>('command-execution-started', (event) => {
                console.debug('[CMD EXEC] Started:', event.payload);

                const isNative = event.payload.shell === false
                    && typeof event.payload.program === 'string'
                    && event.payload.program.length > 0
                    && (event.payload.blocking ?? true);
                const reservedTerminal = isNative
                    ? {
                        id: event.payload.call_id,
                        title: event.payload.program || 'Command',
                    }
                    : reserveTerminalForCommand(event.payload.call_id, event.payload.cwd);
                const pending: PendingCommand = {
                    callId: event.payload.call_id,
                    terminalId: reservedTerminal.id,
                    terminalTitle: reservedTerminal.title,
                    command: event.payload.command,
                    program: event.payload.program,
                    args: event.payload.args,
                    native: isNative,
                    shell: event.payload.shell,
                    cwd: event.payload.cwd,
                    blocking: event.payload.blocking ?? true,
                    waitMsBeforeAsync: event.payload.wait_ms_before_async,
                    started: false,
                    fallbackAttempted: false,
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
                if (pending.native) {
                    void executeNativeCommand(pending);
                } else {
                    void sendCommandToBlade(pending);
                }
            });

            unlistenTerminalReady = await listen<{ id: string }>('terminal-ready', (event) => {
                const terminalId = event.payload.id;
                updateTerminal(terminalId, terminal => terminal ? { ...terminal, ready: true, opening: false } : terminal);
                resolveTerminalReady(terminalId);
            });

            unlistenCmdDetected = await listen<{ terminal_id: string; call_id: string }>('blade-cmd-started', () => {
                // No-op: command-execution-started already marked command active and sent input.
            });

            unlistenCmdExitDetected = await listen<{ terminal_id: string; call_id: string; exit_code: number; output: string }>('blade-cmd-exited', (event) => {
                const { call_id: callId, exit_code: exitCode, output } = event.payload;
                const pending = pendingCommandsRef.current.get(callId);
                if (!pending) return;
                pendingCommandsRef.current.delete(callId);
                if (!pending.native) {
                    releaseTerminalReservation(pending, output.includes(DETACHED_MARKER));
                }
                handleCommandComplete(callId, output, exitCode);
            });

            unlistenExit = await listen<BladeEventEnvelope>('blade-event', (event) => {
                const envelope = event.payload;
                if (envelope.event.type !== 'Terminal') {
                    return;
                }

                const terminalEvent = envelope.event.payload;
                if (terminalEvent.type !== 'Exit') {
                    return;
                }

                const { id: terminalId, code: exitCode } = terminalEvent.payload;
                invalidateTerminal(terminalId);
                resolveTerminalReady(terminalId);
                const pending = Array.from(pendingCommandsRef.current.values()).find(
                    cmd => cmd.terminalId === terminalId,
                );
                if (pending) {
                    pendingCommandsRef.current.delete(pending.callId);
                    if (!pending.native) {
                        releaseTerminalReservation(pending, false);
                    }
                    handleCommandComplete(
                        pending.callId,
                        `Command terminal exited before sentinel completion (exit ${exitCode}).`,
                        exitCode,
                    );
                }
            });
        };

        setupListeners();

        return () => {
            if (unlistenStart) unlistenStart();
            if (unlistenTerminalReady) unlistenTerminalReady();
            if (unlistenExit) unlistenExit();
            if (unlistenCmdDetected) unlistenCmdDetected();
            if (unlistenCmdExitDetected) unlistenCmdExitDetected();
        };
    }, [executeNativeCommand, handleCommandComplete, invalidateTerminal, releaseTerminalReservation, reserveTerminalForCommand, resolveTerminalReady, sendCommandToBlade, updateTerminal]);

    return {
        executions,
        handleCommandComplete,
        stopCommandExecution,
    };
}
