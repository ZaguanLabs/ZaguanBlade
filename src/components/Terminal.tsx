"use client";

import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { invoke } from "@tauri-apps/api/core"; // Still needed for get_current_workspace
import { listen } from "@tauri-apps/api/event";
import { BladeDispatcher } from "../services/blade";

interface TerminalProps {
    id?: string;
}

export default function Terminal({ id = "main-terminal" }: TerminalProps) {
    const terminalRef = useRef<HTMLDivElement>(null);
    const xtermRef = useRef<XTerm | null>(null);
    const fitAddonRef = useRef<FitAddon | null>(null);

    useEffect(() => {
        if (!terminalRef.current) return;

        // prevent double init
        if (xtermRef.current) return;

        // 1. Initialize xterm
        const term = new XTerm({
            cursorBlink: true,
            fontFamily: "monospace",
            fontSize: 14,
            theme: {
                background: "#1e1e1e",
            },
        });

        const fitAddon = new FitAddon();
        term.loadAddon(fitAddon);

        term.open(terminalRef.current);
        fitAddon.fit();

        xtermRef.current = term;
        fitAddonRef.current = fitAddon;

        // 2. Setup backend PTY
        const initBackend = async () => {
            try {
                // Get workspace root to set as starting directory
                const workspaceRoot = await invoke<string | null>("get_current_workspace");

                await BladeDispatcher.terminal({
                    type: "Spawn",
                    payload: {
                        id,
                        cwd: workspaceRoot || undefined,
                        interactive: true,
                    }
                });

                // Initial resize
                const dims = fitAddon.proposeDimensions();
                if (dims) {
                    await BladeDispatcher.terminal({
                        type: "Resize",
                        payload: { id, rows: dims.rows, cols: dims.cols }
                    });
                }
            } catch (err) {
                console.error("Failed to create terminal:", err);
                term.write("\r\n\x1b[31mFailed to initialize terminal backend.\x1b[0m\r\n");
            }
        };

        initBackend();

        // 3. Listen for output from backend
        const unlistenPromise = listen<{ id: string; data: string }>(
            "terminal-output",
            (event) => {
                if (event.payload.id === id) {
                    term.write(event.payload.data);
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
            if (!fitAddonRef.current || !terminalRef.current) return;
            try {
                fitAddonRef.current.fit();
                const dims = fitAddonRef.current.proposeDimensions();
                if (dims) {
                    console.log("XTerm Resize:", dims);
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
            requestAnimationFrame(handleResize);
        });

        resizeObserver.observe(terminalRef.current);

        return () => {
            // Cleanup
            resizeObserver.disconnect();
            unlistenPromise.then((unlisten) => unlisten());
            term.dispose();
            xtermRef.current = null;
        };
    }, [id]);

    return (
        <div
            ref={terminalRef}
            className="w-full h-full bg-[#1e1e1e]"
            style={{ overflow: "hidden" }}
        />
    );
}
