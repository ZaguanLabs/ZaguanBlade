import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { BladeDispatcher } from "../services/blade";
import { TerminalBuffer } from "../utils/eventBuffer";
import type { BladeEventEnvelope } from "../types/blade";

interface TerminalProps {
    id?: string;
}

export default function Terminal({ id = "main-terminal" }: TerminalProps) {
    const terminalRef = useRef<HTMLDivElement>(null);
    const xtermRef = useRef<XTerm | null>(null);
    const fitAddonRef = useRef<FitAddon | null>(null);
    const terminalBufferRef = useRef<TerminalBuffer | null>(null);

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
        const fitInterval = setInterval(() => {
            fitAttempts++;
            try {
                const dims = fitAddon.proposeDimensions();
                if (dims && dims.cols > 0 && dims.rows > 0) {
                    fitAddon.fit();
                    clearInterval(fitInterval);
                    // Force refresh after successful fit
                    term.refresh(0, term.rows - 1);
                }
            } catch (e) {
                // Ignore errors during layout phase
            }

            // Stop trying after 2 seconds
            if (fitAttempts > 20) {
                clearInterval(fitInterval);
            }
        }, 100);

        xtermRef.current = term;
        fitAddonRef.current = fitAddon;

        // 2. Setup backend PTY
        const initBackend = async () => {
            try {
                const workspaceRoot = await invoke<string | null>("get_current_workspace");

                await BladeDispatcher.terminal({
                    type: "Spawn",
                    payload: {
                        id,
                        cwd: workspaceRoot || undefined,
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
        />
    );
}
