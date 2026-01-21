'use client';
import React, { useEffect, useRef, useState } from 'react';
import { Terminal as XTerm } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { BladeDispatcher } from '../services/blade';
import { Terminal } from 'lucide-react';

interface ChatTerminalProps {
    commandId: string;
    command: string;
    cwd?: string;
    onComplete?: (output: string, exitCode: number) => void;
}

export const ChatTerminal: React.FC<ChatTerminalProps> = ({
    commandId,
    command,
    cwd,
    onComplete
}) => {
    const terminalRef = useRef<HTMLDivElement>(null);
    const xtermRef = useRef<XTerm | null>(null);
    const outputBufferRef = useRef<string>('');
    const isRunningRef = useRef<boolean>(true);
    const [statusText, setStatusText] = useState('Running...');

    // Keep reference to latest callback to avoid effect re-triggers
    const onCompleteRef = useRef(onComplete);
    useEffect(() => {
        onCompleteRef.current = onComplete;
    }, [onComplete]);

    useEffect(() => {
        if (!terminalRef.current) return;
        if (xtermRef.current) return;

        // Initialize xterm with compact settings for chat
        const term = new XTerm({
            cursorBlink: false,
            fontFamily: 'monospace',
            fontSize: 12,
            theme: {
                background: '#0a0a0a',
                foreground: '#e5e5e5',
            },
            rows: 8,
            cols: 80,
            scrollback: 1000,
            disableStdin: false, // Enable interactive input
        });

        term.open(terminalRef.current);

        xtermRef.current = term;

        // Write command header
        term.write(`\x1b[1;36m$ ${command}\x1b[0m\r\n`);

        let unlistenOutput: (() => void) | null = null;
        let unlistenExit: (() => void) | null = null;
        let xtermOnData: { dispose: () => void } | null = null;

        // Execute command via backend
        const executeCommand = async () => {
            try {
                const terminalId = `chat-cmd-${commandId}`;

                // Capture input and forward to backend
                xtermOnData = term.onData((data) => {
                    if (!isRunningRef.current) return;
                    BladeDispatcher.terminal({
                        type: 'Input',
                        payload: {
                            id: terminalId,
                            data: data
                        }
                    }).catch(err => console.error('Failed to send input:', err));
                });

                // Listen for output
                unlistenOutput = await listen<{ id: string; data: string }>(
                    'terminal-output',
                    (event) => {
                        if (event.payload.id === terminalId) {
                            term.write(event.payload.data);
                            outputBufferRef.current += event.payload.data;
                        }
                    }
                );

                // Listen for exit
                unlistenExit = await listen<{ id: string; exit_code: number }>(
                    'terminal-exit',
                    (event) => {
                        if (event.payload.id === terminalId) {
                            isRunningRef.current = false;
                            const exitCode = event.payload.exit_code;

                            if (exitCode === 0) {
                                term.write(`\r\n\x1b[1;32m✓ Command completed successfully\x1b[0m\r\n`);
                                setStatusText('Completed');
                            } else {
                                term.write(`\r\n\x1b[1;31m✗ Command failed with exit code ${exitCode}\x1b[0m\r\n`);
                                setStatusText(`Failed (exit ${exitCode})`);
                            }

                            if (onCompleteRef.current) {
                                onCompleteRef.current(outputBufferRef.current, exitCode);
                            }

                            if (unlistenOutput) unlistenOutput();
                            if (unlistenExit) unlistenExit();
                            if (xtermOnData) xtermOnData.dispose();
                        }
                    }
                );

                // Execute the command via Blade Protocol (Spawn, interactive)
                await BladeDispatcher.terminal({
                    type: 'Spawn',
                    payload: {
                        id: terminalId,
                        command,
                        cwd: cwd || undefined,
                        interactive: true // Enable interactive mode
                    }
                });

            } catch (err) {
                console.error('Failed to execute command:', err);
                term.write(`\r\n\x1b[31mFailed to execute command: ${err}\x1b[0m\r\n`);
                isRunningRef.current = false;
                setStatusText('Error');
                if (onCompleteRef.current) {
                    onCompleteRef.current(outputBufferRef.current, 1);
                }
            }
        };

        executeCommand();

        return () => {
            if (unlistenOutput) unlistenOutput();
            if (unlistenExit) unlistenExit();
            if (xtermOnData) xtermOnData.dispose();
            term.dispose();
            xtermRef.current = null;
        };
    }, [commandId, command, cwd]);

    return (
        <div className="my-2 border border-zinc-800 rounded-lg overflow-hidden bg-[#0a0a0a]">
            {/* Header */}
            <div className="flex items-center justify-between px-3 py-2 bg-zinc-900/50 border-b border-zinc-800">
                <div className="flex items-center gap-2">
                    <Terminal className="w-3.5 h-3.5 text-emerald-500" />
                    <span className="text-xs font-mono text-zinc-400">
                        {statusText}
                    </span>
                </div>
            </div>

            {/* Terminal - Fixed height to prevent flickering */}
            <div
                ref={terminalRef}
                className="w-full"
                style={{
                    height: '200px',
                    overflow: 'hidden'
                }}
            />
        </div>
    );
};
