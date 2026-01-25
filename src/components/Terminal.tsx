import { useEffect, useRef, useCallback } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { invoke } from "@tauri-apps/api/core";
import { listen, emit } from "@tauri-apps/api/event";
import { BladeDispatcher } from "../services/blade";
import { TerminalBuffer } from "../utils/eventBuffer";
import type { BladeEventEnvelope } from "../types/blade";
import { useContextMenu, ContextMenuItem } from "./ui/ContextMenu";
import { Copy, ClipboardPaste, Trash2, MessageSquare } from "lucide-react";

interface TerminalProps {
    id?: string;
    cwd?: string;
}

export default function Terminal({ id = "main-terminal", cwd }: TerminalProps) {
    const terminalRef = useRef<HTMLDivElement>(null);
    const xtermRef = useRef<XTerm | null>(null);
    const fitAddonRef = useRef<FitAddon | null>(null);
    const terminalBufferRef = useRef<TerminalBuffer | null>(null);
    const fitIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
    const { showMenu } = useContextMenu();

    // Context menu handler
    const handleContextMenu = useCallback((e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();

        const term = xtermRef.current;
        const selection = term?.getSelection() || '';

        const items: ContextMenuItem[] = [
            {
                id: 'copy',
                label: 'Copy',
                icon: <Copy className="w-4 h-4" />,
                shortcut: 'Ctrl+Shift+C',
                disabled: !selection,
                onClick: async () => {
                    if (selection) {
                        try {
                            await navigator.clipboard.writeText(selection);
                        } catch (err) {
                            console.error('Failed to copy:', err);
                        }
                    }
                }
            },
            {
                id: 'paste',
                label: 'Paste',
                icon: <ClipboardPaste className="w-4 h-4" />,
                shortcut: 'Ctrl+Shift+V',
                onClick: async () => {
                    try {
                        const text = await navigator.clipboard.readText();
                        if (text && term) {
                            BladeDispatcher.terminal({
                                type: "Input",
                                payload: { id, data: text }
                            }).catch(console.error);
                        }
                    } catch (err) {
                        console.error('Failed to paste:', err);
                    }
                }
            },
            { id: 'div-1', label: '', divider: true },
            {
                id: 'clear',
                label: 'Clear Terminal',
                icon: <Trash2 className="w-4 h-4" />,
                onClick: () => {
                    if (term) {
                        term.clear();
                    }
                }
            },
            { id: 'div-2', label: '', divider: true },
            {
                id: 'send-to-chat',
                label: 'Send to Chat',
                icon: <MessageSquare className="w-4 h-4" />,
                disabled: !selection,
                onClick: async () => {
                    if (selection) {
                        // Emit event to send selection to chat input
                        await emit('terminal-to-chat', { text: selection });
                    }
                }
            },
        ];

        showMenu({ x: e.clientX, y: e.clientY }, items);
    }, [id, showMenu]);

    useEffect(() => {
        if (!terminalRef.current) return;
        if (xtermRef.current) return;

        // 1. Initialize xterm with Design System tokens
        const term = new XTerm({
            cursorBlink: true,
            fontFamily: "\"Fira Code\", \"Symbols Nerd Font Mono\", monospace",
            fontSize: 12,
            lineHeight: 1.2,
            theme: {
                background: "#09090b",
                foreground: "#d4d4d8",
                cursor: "#e4e4e7",
                selectionBackground: "rgba(255, 255, 255, 0.2)",
            },
            allowTransparency: true,
            fontWeight: "normal",
            fontWeightBold: "bold",
        });

        const fitAddon = new FitAddon();
        term.loadAddon(fitAddon);
        term.loadAddon(new WebLinksAddon());

        // Note: Removed WebGL addon due to stability issues in Linux production builds (WebKitGTK).
        // Sticking to the default canvas renderer which is more robust.

        term.open(terminalRef.current);

        // Robust Fit Strategy:
        // PTY spawning needs dimensions. We must ensure the terminal has size before fitting.
        // We poll for a short period until we get valid dimensions.
        let fitAttempts = 0;
        fitIntervalRef.current = setInterval(() => {
            fitAttempts++;
            try {
                const dims = fitAddon.proposeDimensions();
                if (dims && dims.cols > 0 && dims.rows > 0) {
                    fitAddon.fit();
                    if (fitIntervalRef.current) {
                        clearInterval(fitIntervalRef.current);
                        fitIntervalRef.current = null;
                    }
                    // Force refresh after successful fit
                    term.refresh(0, term.rows - 1);
                }
            } catch (e) {
                // Ignore errors during layout phase
            }

            // Stop trying after 2 seconds
            if (fitAttempts > 20) {
                if (fitIntervalRef.current) {
                    clearInterval(fitIntervalRef.current);
                    fitIntervalRef.current = null;
                }
            }
        }, 100);

        xtermRef.current = term;
        fitAddonRef.current = fitAddon;

        // 2. Setup backend PTY
        const initBackend = async () => {
            try {
                // Use provided cwd or fall back to workspace root
                let terminalCwd = cwd;
                if (!terminalCwd) {
                    const workspaceRoot = await invoke<string | null>("get_current_workspace");
                    terminalCwd = workspaceRoot || undefined;
                }

                await BladeDispatcher.terminal({
                    type: "Spawn",
                    payload: {
                        id,
                        cwd: terminalCwd,
                        interactive: true,
                    }
                });

                // Initial resize after backend spawn
                setTimeout(() => {
                    const dims = fitAddon.proposeDimensions();
                    if (dims) {
                        BladeDispatcher.terminal({
                            type: "Resize",
                            payload: { id, rows: dims.rows, cols: dims.cols }
                        });
                    }
                }, 50);
            } catch (err) {
                console.error("Failed to create terminal:", err);
                term.write("\r\n\x1b[31mFailed to initialize terminal backend.\x1b[0m\r\n");
            }
        };

        initBackend();

        // Initialize v1.1 terminal buffer
        if (!terminalBufferRef.current) {
            terminalBufferRef.current = new TerminalBuffer(
                (termId, data) => {
                    if (termId === id && xtermRef.current) {
                        xtermRef.current.write(data);
                    }
                }
            );
        }

        // 3. Listen for output from backend (legacy)
        const unlistenLegacy = listen<{ id: string; data: string }>(
            "terminal-output",
            (event) => {
                if (event.payload.id === id) {
                    term.write(event.payload.data);
                }
            }
        );

        // v1.1: Listen for blade-event with sequence numbers
        const unlistenV11 = listen<BladeEventEnvelope>(
            "blade-event",
            (event) => {
                const envelope = event.payload;

                if (envelope.event.type === 'Terminal') {
                    const terminalEvent = envelope.event.payload;

                    if (terminalEvent.type === 'Output') {
                        const { id: termId, seq, data } = terminalEvent.payload;
                        if (terminalBufferRef.current) {
                            terminalBufferRef.current.addOutput(termId, seq, data);
                        }
                    } else if (terminalEvent.type === 'Spawned') {
                        const { id: termId, owner } = terminalEvent.payload;
                        console.log(`[v1.1 Terminal] Spawned: id=${termId}, owner=${owner.type}`);
                    } else if (terminalEvent.type === 'Exit') {
                        const { id: termId, code } = terminalEvent.payload;
                        if (termId === id && xtermRef.current) {
                            xtermRef.current.write(`\r\n\x1b[33mProcess exited with code ${code}\x1b[0m\r\n`);
                        }
                    }
                }
            }
        );

        // 4. Send input to backend
        term.onData((data) => {
            BladeDispatcher.terminal({
                type: "Input",
                payload: { id, data }
            }).catch(console.error);
        });

        // 5. Handle Resize
        const handleResize = () => {
            if (!fitAddonRef.current || !terminalRef.current || !xtermRef.current) return;
            try {
                fitAddonRef.current.fit();

                // Force a refresh of the renderer
                xtermRef.current.refresh(0, xtermRef.current.rows - 1);

                const dims = fitAddonRef.current.proposeDimensions();
                if (dims && dims.cols > 0 && dims.rows > 0) {
                    BladeDispatcher.terminal({
                        type: "Resize",
                        payload: { id, rows: dims.rows, cols: dims.cols }
                    }).catch(e => console.error("Resize failed", e));
                }
            } catch (e) {
                console.error("Resize logic error", e);
            }
        };

        const resizeObserver = new ResizeObserver(() => {
            // Debounce or RAF
            requestAnimationFrame(() => {
                // Check if visible
                if (terminalRef.current?.offsetParent) {
                    handleResize();
                }
            });
        });

        resizeObserver.observe(terminalRef.current);

        return () => {
            // Clear the fit interval if still running
            if (fitIntervalRef.current) {
                clearInterval(fitIntervalRef.current);
                fitIntervalRef.current = null;
            }

            resizeObserver.disconnect();
            unlistenLegacy.then((unlisten) => unlisten());
            unlistenV11.then((unlisten) => unlisten());

            // Dispose logic
            try {
                // We don't dispose the term immediately to avoid race conditions if the ref is used elsewhere?
                // No, we should dispose.
                term.dispose();
            } catch (e) { console.error("Error disposing terminal", e); }

            xtermRef.current = null;
            terminalBufferRef.current = null;
        };
    }, [id]);

    return (
        <div
            ref={terminalRef}
            className="w-full h-full bg-[var(--term-bg)]"
            style={{ overflow: "hidden" }}
            onContextMenu={handleContextMenu}
        />
    );
}
