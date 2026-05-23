import { useEffect, useState, useRef, useCallback } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import { subscribeBladeNestedEventType } from '../services/bladeEvents';
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

type BladeCmdExitedPayload = {
    terminal_id: string;
    call_id: string;
    exit_code: number;
    output: string;
};

const DETACHED_MARKER = '[run_command] detached pid=';
const TERMINAL_READY_TIMEOUT_MS = 15000;

const buildCommandTerminalId = (callId: string) => `ai-cmd-${callId}`;

const escapeShellArgValue = (value: string) => {
    if (value.length === 0) return "''";
    return `'${value.replace(/'/g, `'"'"'`)}'`;
};

export function buildRunCommandTerminalSpawnCommand(
    callId: string,
    commandToRun: string,
    blocking = true,
    waitMsBeforeAsync?: number,
) {
    const escapedCallId = escapeShellArgValue(callId);
    const startSentinel = `printf '\\n##BLADE_CMD_START:%s##\\n' ${escapedCallId}`;
    const exitSentinel = `printf '\\n##BLADE_CMD_EXIT:%s:%s##\\n' ${escapedCallId} "$__blade_ec"`;

    if (blocking) {
        return [
            startSentinel,
            `( ${commandToRun} )`,
            '__blade_ec=$?',
            exitSentinel,
            'unset __blade_ec',
        ].join('; ');
    }

    const waitMs = typeof waitMsBeforeAsync === 'number'
        ? Math.max(0, waitMsBeforeAsync)
        : 1000;
    const waitSeconds = (waitMs / 1000).toString();

    return [
        startSentinel,
        `( ${commandToRun} ) & __blade_pid=$!`,
        waitMs > 0 ? `sleep ${waitSeconds}` : null,
        `if kill -0 "$__blade_pid" 2>/dev/null; then disown "$__blade_pid" 2>/dev/null || true; echo '${DETACHED_MARKER}'"$__blade_pid"; __blade_ec=0; ${exitSentinel}; unset __blade_ec __blade_pid; return 0 2>/dev/null || true; fi`,
        'wait "$__blade_pid"',
        '__blade_ec=$?',
        exitSentinel,
        'unset __blade_ec __blade_pid',
    ].filter(Boolean).join('; ');
}

export function useCommandExecution() {
    const [executions, setExecutions] = useState<Map<string, CommandExecution>>(new Map());
    const pendingCommandsRef = useRef<Map<string, PendingCommand>>(new Map());
    const commandOutputBuffersRef = useRef<Map<string, string>>(new Map());
    const activeTerminalCommandRef = useRef<Map<string, string>>(new Map());
    const terminalsRef = useRef<Map<string, ManagedTerminal>>(new Map());
    const terminalReadyWaitersRef = useRef<Map<string, Array<() => void>>>(new Map());
    const terminalExitFallbackTimersRef = useRef<Map<string, number>>(new Map());
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
        return escapeShellArgValue(value);
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
        waitForReady = true,
        displayCommand?: string,
    ) => {
        const existing = terminalsRef.current.get(terminalId);
        if (existing?.ready) {
            await emit('open-terminal', {
                id: terminalId,
                cwd: cwd ?? existing.cwd,
                title: existing.title ?? title,
                interactive: true,
                focus: true,
                transient: terminalId !== BLADE_TERMINAL_ID,
                displayCommand,
            });
            if (command) {
                await BladeDispatcher.terminal({
                    type: 'Input',
                    payload: { id: terminalId, data: `${command}\n` },
                });
            }
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
                interactive: true,
                focus: true,
                transient: terminalId !== BLADE_TERMINAL_ID,
            });
        }

        if (waitForReady) {
            await waitForTerminalReady(terminalId);
        }

        if (command) {
            await BladeDispatcher.terminal({
                type: 'Input',
                payload: { id: terminalId, data: `${command}\n` },
            });
        }
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
        // Try to reuse the primary terminal first
        let terminal = terminalsRef.current.get(BLADE_TERMINAL_ID);
        
        // If the primary doesn't exist, is busy, or is locked in background
        if (!terminal || terminal.activeCallId || terminal.backgroundLocked) {
            // Find another idle terminal
            const idleTerminal = Array.from(terminalsRef.current.values()).find(t => 
                !t.primary && !t.activeCallId && !t.backgroundLocked && t.id.startsWith('ai-cmd-')
            );
            
            if (idleTerminal) {
                terminal = idleTerminal;
            } else {
                // If all are busy, create a new one
                terminal = createCommandTerminal(callId, cwd);
            }
        }

        updateTerminal(terminal.id, current => {
            const base = current ?? terminal!;
            return {
                ...base,
                cwd: cwd ?? base.cwd,
                activeCallId: callId,
            };
        });

        return terminalsRef.current.get(terminal!.id) ?? terminal!;
    }, [createCommandTerminal, updateTerminal]);

    const clearTerminalExitFallback = useCallback((callId: string) => {
        const timeoutId = terminalExitFallbackTimersRef.current.get(callId);
        if (typeof timeoutId === 'number') {
            window.clearTimeout(timeoutId);
            terminalExitFallbackTimersRef.current.delete(callId);
        }
    }, []);

    const takeCommandOutputBuffer = useCallback((callId: string, output = '') => {
        const buffered = commandOutputBuffersRef.current.get(callId) ?? '';
        commandOutputBuffersRef.current.delete(callId);

        if (output.trim().length > 0) {
            return output;
        }

        return buffered;
    }, []);

    const buildTerminalSpawnCommand = useCallback((pending: PendingCommand) => {
        const commandToRun = buildCommandForExecution(pending);
        return buildRunCommandTerminalSpawnCommand(
            pending.callId,
            commandToRun,
            pending.blocking,
            pending.waitMsBeforeAsync,
        );
    }, [buildCommandForExecution]);

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
        const command = buildTerminalSpawnCommand(pending);

        try {
            commandOutputBuffersRef.current.delete(pending.callId);
            activeTerminalCommandRef.current.set(pending.terminalId, pending.callId);

            await openManagedTerminal(
                pending.terminalId,
                pending.terminalTitle,
                pending.cwd,
                command,
                true,
                true,
                pending.command,
            );
        } catch (err) {
            activeTerminalCommandRef.current.delete(pending.terminalId);
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
    }, [buildTerminalSpawnCommand, createCommandTerminal, handleCommandComplete, invalidateTerminal, openManagedTerminal, releaseTerminalReservation]);

    const stopCommandExecution = useCallback(async (callId: string) => {
        const pending = pendingCommandsRef.current.get(callId);
        if (!pending) return;

        pendingCommandsRef.current.delete(callId);
        clearTerminalExitFallback(callId);
        commandOutputBuffersRef.current.delete(callId);
        activeTerminalCommandRef.current.delete(pending.terminalId);

        try {
            if (pending.native) {
                await invoke<boolean>('cancel_command_execution', {
                    callId,
                });
            } else {
                await BladeDispatcher.terminal({
                    type: 'Kill',
                    payload: {
                        id: pending.terminalId,
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
    }, [clearTerminalExitFallback, handleCommandComplete, releaseTerminalReservation]);

    useEffect(() => {
        let unlistenStart: (() => void) | undefined;
        let unlistenTerminalReady: (() => void) | undefined;
        let unlistenBladeCmdExited: (() => void) | undefined;
        let unlistenExit: (() => void) | undefined;
        let unsubscribeTerminalOutput: (() => void) | undefined;

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

                const reservedTerminal = reserveTerminalForCommand(event.payload.call_id, event.payload.cwd);
                const pending: PendingCommand = {
                    callId: event.payload.call_id,
                    terminalId: reservedTerminal.id,
                    terminalTitle: reservedTerminal.title,
                    command: event.payload.command,
                    program: event.payload.program,
                    args: event.payload.args,
                    native: false,
                    shell: event.payload.shell,
                    cwd: event.payload.cwd,
                    blocking: event.payload.blocking ?? true,
                    waitMsBeforeAsync: event.payload.wait_ms_before_async,
                    started: false,
                    fallbackAttempted: false,
                };
                commandOutputBuffersRef.current.delete(event.payload.call_id);
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
                updateTerminal(terminalId, terminal => {
                    if (terminal) {
                        return { ...terminal, ready: true, opening: false };
                    }

                    return {
                        id: terminalId,
                        title: terminalId === BLADE_TERMINAL_ID ? BLADE_TERMINAL_TITLE : terminalId,
                        ready: true,
                        opening: false,
                        activeCallId: null,
                        backgroundLocked: false,
                        primary: terminalId === BLADE_TERMINAL_ID,
                    };
                });
                resolveTerminalReady(terminalId);
            });

            unlistenBladeCmdExited = await listen<BladeCmdExitedPayload>('blade-cmd-exited', (event) => {
                const { terminal_id: terminalId, call_id: callId, exit_code: exitCode, output } = event.payload;
                const pending = pendingCommandsRef.current.get(callId);
                if (!pending || pending.terminalId !== terminalId) {
                    return;
                }

                clearTerminalExitFallback(callId);
                pendingCommandsRef.current.delete(callId);
                activeTerminalCommandRef.current.delete(terminalId);
                releaseTerminalReservation(pending, !pending.blocking && output.includes(DETACHED_MARKER));
                commandOutputBuffersRef.current.delete(callId);
                void handleCommandComplete(callId, output, exitCode);
            });

            unsubscribeTerminalOutput = subscribeBladeNestedEventType('Terminal', 'Output', (payload) => {
                const callId = activeTerminalCommandRef.current.get(payload.id);
                if (!callId) {
                    return;
                }

                const existing = commandOutputBuffersRef.current.get(callId) ?? '';
                const nextOutput = existing + payload.data;
                commandOutputBuffersRef.current.set(callId, nextOutput);

                const pending = pendingCommandsRef.current.get(callId);
                if (!pending || pending.native || pending.blocking || !nextOutput.includes(DETACHED_MARKER)) {
                    return;
                }

                clearTerminalExitFallback(callId);
                pendingCommandsRef.current.delete(callId);
                activeTerminalCommandRef.current.delete(payload.id);
                releaseTerminalReservation(pending, true);
                const finalOutput = takeCommandOutputBuffer(callId, nextOutput);
                void handleCommandComplete(callId, finalOutput, 0);
            });

            unlistenExit = subscribeBladeNestedEventType('Terminal', 'Exit', (payload) => {
                const { id: terminalId, code: exitCode } = payload;
                invalidateTerminal(terminalId);
                resolveTerminalReady(terminalId);
                const pending = Array.from(pendingCommandsRef.current.values()).find(
                    cmd => cmd.terminalId === terminalId,
                );
                if (pending) {
                    clearTerminalExitFallback(pending.callId);
                    const timeoutId = window.setTimeout(() => {
                        terminalExitFallbackTimersRef.current.delete(pending.callId);
                        const latestPending = pendingCommandsRef.current.get(pending.callId);
                        if (!latestPending || latestPending.terminalId !== terminalId) {
                            return;
                        }
                        pendingCommandsRef.current.delete(latestPending.callId);
                        activeTerminalCommandRef.current.delete(terminalId);
                        if (!latestPending.native) {
                            releaseTerminalReservation(latestPending, false);
                        }
                        const fallbackOutput = takeCommandOutputBuffer(latestPending.callId);
                        handleCommandComplete(
                            latestPending.callId,
                            fallbackOutput.trim().length > 0
                                ? fallbackOutput
                                : `Command terminal exited before completion (exit ${exitCode}).`,
                            exitCode,
                        );
                    }, 150);
                    terminalExitFallbackTimersRef.current.set(pending.callId, timeoutId);
                }
            });
        };

        setupListeners();
        const terminalExitFallbackTimers = terminalExitFallbackTimersRef.current;
        const activeTerminalCommand = activeTerminalCommandRef.current;
        const commandOutputBuffers = commandOutputBuffersRef.current;

        return () => {
            for (const timeoutId of terminalExitFallbackTimers.values()) {
                window.clearTimeout(timeoutId);
            }
            terminalExitFallbackTimers.clear();
            if (unlistenStart) unlistenStart();
            if (unlistenTerminalReady) unlistenTerminalReady();
            if (unlistenBladeCmdExited) unlistenBladeCmdExited();
            if (unlistenExit) unlistenExit();
            if (unsubscribeTerminalOutput) unsubscribeTerminalOutput();
            activeTerminalCommand.clear();
            commandOutputBuffers.clear();
        };
    }, [clearTerminalExitFallback, executeNativeCommand, handleCommandComplete, invalidateTerminal, releaseTerminalReservation, reserveTerminalForCommand, resolveTerminalReady, sendCommandToBlade, takeCommandOutputBuffer, updateTerminal]);

    return {
        executions,
        handleCommandComplete,
        stopCommandExecution,
    };
}
