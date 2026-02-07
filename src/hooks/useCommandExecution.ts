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

// Plain-text sentinel markers — NO escape characters.
// These are detected and stripped by the Rust terminal reader thread (terminal.rs)
// before the output reaches xterm, so they never appear in the terminal display.
// The Rust side emits dedicated events when it detects these sentinels.
const SENTINEL_START = '##BLADE_CMD_START:';
const SENTINEL_EXIT = '##BLADE_CMD_EXIT:';
const SENTINEL_END = '##';

export function useCommandExecution() {
    const [executions, setExecutions] = useState<Map<string, CommandExecution>>(new Map());
    const pendingCommandsRef = useRef<Map<string, PendingCommand>>(new Map());
    const activeCallIdRef = useRef<string | null>(null);
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

    const sendCommandToBlade = useCallback((pending: PendingCommand) => {
        const { callId, command, cwd } = pending;

        // Build the command to send to the interactive terminal.
        //
        // We use plain-text sentinel markers (no escape characters) to detect
        // command start/end in the terminal output stream. The Rust terminal
        // reader thread strips these sentinels before they reach xterm display
        // and emits dedicated events for command tracking.
        //
        // The sentinels are emitted via `echo` to stdout so they appear in the
        // PTY output stream. The shell echoes the command line, but the Rust
        // reader strips the sentinel echo lines too.

        const parts: string[] = [];
        // Emit start sentinel to stdout (will be stripped by Rust)
        parts.push(`echo '${SENTINEL_START}${callId}${SENTINEL_END}'`);
        if (cwd) {
            parts.push(`cd ${escapeShellArg(cwd)}`);
        }
        parts.push(command);
        // Capture exit code and emit exit sentinel
        parts.push(`__blade_ec=$?; echo '${SENTINEL_EXIT}${callId}:'"$__blade_ec"'${SENTINEL_END}'; exit $__blade_ec`);
        // Use semicolons to join — this becomes a single shell command line
        // Wrap in a subshell so `exit` doesn't kill the interactive shell
        const innerCmd = parts.join('; ');
        const payload = `( ${innerCmd} )\n`;

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
                    commandId: event.payload.command_id,
                    callId: event.payload.call_id,
                    command: event.payload.command,
                    cwd: event.payload.cwd,
                    output: '',
                    started: false,
                };
                pendingCommandsRef.current.set(event.payload.call_id, pending);

                // Set active call immediately so output accumulation starts
                // before the blade-cmd-started event arrives from Rust
                pending.started = true;
                activeCallIdRef.current = event.payload.call_id;

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

            // Listen for sentinel-detected events from Rust terminal reader
            // (confirmation only — activeCallIdRef is already set in command-execution-started)
            unlistenCmdDetected = await listen<{ terminal_id: string; call_id: string }>('blade-cmd-started', (_event) => {
                // No-op: output accumulation is already active
            });

            unlistenCmdExitDetected = await listen<{ terminal_id: string; call_id: string; exit_code: number; output: string }>('blade-cmd-exited', (event) => {
                if (event.payload.terminal_id !== BLADE_TERMINAL_ID) return;
                const { call_id: callId, exit_code: exitCode, output: cmdOutput } = event.payload;
                const pending = pendingCommandsRef.current.get(callId);
                if (pending) {
                    // Use output accumulated in Rust (reliable, no race condition)
                    handleCommandComplete(callId, cmdOutput, exitCode);
                    pendingCommandsRef.current.delete(callId);
                    if (activeCallIdRef.current === callId) {
                        activeCallIdRef.current = null;
                    }
                }
            });

            // terminal-output events are used by Terminal.tsx for display.
            // Output accumulation for command results is handled in Rust.
            unlistenOutput = undefined;

            unlistenExit = await listen<{ id: string; exit_code: number }>('terminal-exit', (event) => {
                if (event.payload.id === BLADE_TERMINAL_ID) {
                    bladeReadyRef.current = false;
                }
            });
        };

        setupListeners();

        return () => {
            if (unlistenStart) unlistenStart();
            if (unlistenOutput) unlistenOutput();
            if (unlistenExit) unlistenExit();
            if (unlistenBlade) unlistenBlade();
            if (unlistenCmdDetected) unlistenCmdDetected();
            if (unlistenCmdExitDetected) unlistenCmdExitDetected();
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
