import { useEffect, useRef, useCallback } from "react";
import { useTranslation } from 'react-i18next';
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { BladeDispatcher } from "../services/blade";
import { subscribeBladeNestedEventType } from "../services/bladeEvents";
import { TerminalBuffer } from "../utils/eventBuffer";
import { useTheme } from "../contexts/ThemeContext";
import { useContextMenu, ContextMenuItem } from "./ui/ContextMenu";
import { Copy, ClipboardPaste, Trash2, MessageSquare } from "lucide-react";
import { BLADE_TERMINAL_ID } from "../constants/terminal";
import { recordDebugPerf } from "../utils/debugPerf";

function stripAnsiEscapes(data: string): string {
    return data.replace(/\u001b\[[0-?]*[ -\/]*[@-~]/g, '').replace(/\u001b\][^\u0007]*(?:\u0007|\u001b\\)/g, '');
}

/**
 * Strip any remaining BLADE sentinel artifacts from terminal output.
 * The Rust terminal reader thread handles the primary sentinel stripping,
 * but the v1.1 blade-event path bypasses that, so we catch stragglers here.
 */
function sanitizeTerminalOutput(data: string): string {
    return data
        .split(/(\r?\n)/)
        .reduce<string[]>((parts, segment, index, source) => {
            if (segment === '\n' || segment === '\r\n') {
                const previous = source[index - 1];
                if (previous && previous.length > 0) {
                    parts.push(segment);
                }
                return parts;
            }

            const line = segment;
            const plainLine = stripAnsiEscapes(line);
            const trimmed = plainLine.trim();
            const isBladeWrapperLine = trimmed.includes('##BLADE_CMD_')
                || trimmed.includes('__blade_ec')
                || trimmed.includes('__blade_pid')
                || trimmed.includes('unset __blade_ec __blade_pid')
                || trimmed.includes('[run_command] detached pid=')
                || /(?:^|\s)(?:[%#$][ ]*)?(?:\(?\s*)?(?:echo|printf)\s+'/.test(trimmed);

            if (!isBladeWrapperLine) {
                parts.push(line);
            }

            return parts;
        }, [])
        .join('');
}

interface TerminalTheme {
    background: string;
    foreground: string;
    cursor: string;
    cursorAccent: string;
    selectionBackground: string;
    black: string;
    red: string;
    green: string;
    yellow: string;
    blue: string;
    magenta: string;
    cyan: string;
    white: string;
    brightBlack: string;
    brightRed: string;
    brightGreen: string;
    brightYellow: string;
    brightBlue: string;
    brightMagenta: string;
    brightCyan: string;
    brightWhite: string;
}

// Cache terminal theme to avoid repeated getComputedStyle calls
let cachedTerminalTheme: TerminalTheme | null = null;

function getTerminalTheme(): TerminalTheme {
    if (cachedTerminalTheme) {
        return cachedTerminalTheme;
    }

    const styles = getComputedStyle(document.documentElement);

    cachedTerminalTheme = {
        background: styles.getPropertyValue('--term-bg').trim(),
        foreground: styles.getPropertyValue('--term-fg').trim(),
        cursor: styles.getPropertyValue('--term-cursor').trim(),
        cursorAccent: styles.getPropertyValue('--term-cursor-accent').trim(),
        selectionBackground: styles.getPropertyValue('--term-selection').trim(),
        black: styles.getPropertyValue('--term-black').trim(),
        red: styles.getPropertyValue('--term-red').trim(),
        green: styles.getPropertyValue('--term-green').trim(),
        yellow: styles.getPropertyValue('--term-yellow').trim(),
        blue: styles.getPropertyValue('--term-blue').trim(),
        magenta: styles.getPropertyValue('--term-magenta').trim(),
        cyan: styles.getPropertyValue('--term-cyan').trim(),
        white: styles.getPropertyValue('--term-white').trim(),
        brightBlack: styles.getPropertyValue('--term-bright-black').trim(),
        brightRed: styles.getPropertyValue('--term-bright-red').trim(),
        brightGreen: styles.getPropertyValue('--term-bright-green').trim(),
        brightYellow: styles.getPropertyValue('--term-bright-yellow').trim(),
        brightBlue: styles.getPropertyValue('--term-bright-blue').trim(),
        brightMagenta: styles.getPropertyValue('--term-bright-magenta').trim(),
        brightCyan: styles.getPropertyValue('--term-bright-cyan').trim(),
        brightWhite: styles.getPropertyValue('--term-bright-white').trim(),
    };

    return cachedTerminalTheme;
}

interface TerminalProps {
    id?: string;
    cwd?: string;
    command?: string;
    displayCommand?: string;
    interactive?: boolean;
}

export const Terminal: React.FC<TerminalProps> = ({ id = "main-terminal", cwd, command, displayCommand, interactive = true }) => {
    recordDebugPerf(`Terminal.render.${id}`);
    const { t } = useTranslation();
    const { themeId } = useTheme();
    const terminalRef = useRef<HTMLDivElement>(null);
    const xtermRef = useRef<XTerm | null>(null);
    const fitAddonRef = useRef<FitAddon | null>(null);
    const terminalBufferRef = useRef<TerminalBuffer | null>(null);
    const fitIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
    const resizeFrameRef = useRef<number | null>(null);
    const lastResizeRef = useRef<{ cols: number; rows: number } | null>(null);
    const initialCwdRef = useRef(cwd);
    const initialCommandRef = useRef(command);
    const initialDisplayCommandRef = useRef(displayCommand);
    const initialInteractiveRef = useRef(interactive);
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
                label: t('terminal.copySelection'),
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
                label: t('terminal.paste'),
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
                label: t('terminal.clearTerminal'),
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
                label: t('terminal.sendToChat'),
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
    }, [id, showMenu, t]);

    useEffect(() => {
        if (!terminalRef.current) return;
        if (xtermRef.current) return;

        // 1. Initialize xterm with Tokyo Night theme
        const term = new XTerm({
            cursorBlink: false,
            fontFamily: "\"JetBrains Mono\", \"Symbols Nerd Font Mono\", monospace",
            fontSize: 12,
            lineHeight: 1.2,
            theme: getTerminalTheme(),
            allowTransparency: false,
            fontWeight: "normal",
            fontWeightBold: "bold",
        });

        const fitAddon = new FitAddon();
        term.loadAddon(fitAddon);
        term.loadAddon(new WebLinksAddon());

        // Note: Removed WebGL addon due to stability issues in Linux production builds (WebKitGTK).
        // Sticking to the default canvas renderer which is more robust.

        term.open(terminalRef.current);

        let pasteShortcutInFlight = false;

        // Fix: On Linux, Alt-Gr composed characters (e.g. ~, @, {, [) fire both
        // a keydown and a composition/input event, causing double input. xterm.js
        // only handles AltGraph for Windows in _isThirdLevelShift. We suppress the
        // keydown processing here so the composition path handles it instead.
        term.attachCustomKeyEventHandler((ev: KeyboardEvent) => {
            const isCtrlShift = ev.ctrlKey && ev.shiftKey && !ev.altKey && !ev.metaKey;
            const isCmdShift = ev.metaKey && ev.shiftKey && !ev.altKey && !ev.ctrlKey;
            const isPasteShortcut = (isCtrlShift || isCmdShift) && ev.code === 'KeyV';

            // Handle paste shortcut on keydown only, and swallow corresponding keyup.
            // Some environments still route the shortcut through keyup/default path,
            // which can cause a second paste unless both phases are suppressed.
            if (isPasteShortcut) {
                if (ev.type === 'keyup') {
                    pasteShortcutInFlight = false;
                    return false;
                }

                if (ev.type === 'keydown') {
                    if (pasteShortcutInFlight || ev.repeat) {
                        return false;
                    }
                    pasteShortcutInFlight = true;

                    navigator.clipboard.readText()
                        .then((text) => {
                            if (!text) return;
                            BladeDispatcher.terminal({
                                type: 'Input',
                                payload: { id, data: text },
                            }).catch(console.error);
                        })
                        .catch((err) => {
                            console.error('Failed to paste:', err);
                        });
                    return false;
                }

                return false;
            }

            if (ev.type !== 'keydown') {
                return true;
            }

            if (ev.getModifierState('AltGraph')) {
                return false;
            }

            if ((isCtrlShift || isCmdShift) && ev.code === 'KeyC') {
                const selection = term.getSelection();
                if (selection) {
                    navigator.clipboard.writeText(selection).catch((err) => {
                        console.error('Failed to copy:', err);
                    });
                }
                return false;
            }

            return true;
        });

        const syncTerminalSize = () => {
            recordDebugPerf(`Terminal.syncSize.${id}`);
            if (!fitAddonRef.current || !terminalRef.current || !xtermRef.current) return;
            if (!terminalRef.current.offsetParent) return;
            try {
                const proposed = fitAddonRef.current.proposeDimensions();
                if (!proposed || proposed.cols <= 0 || proposed.rows <= 0) {
                    return;
                }

                const lastResize = lastResizeRef.current;
                if (lastResize && lastResize.cols === proposed.cols && lastResize.rows === proposed.rows) {
                    return;
                }

                fitAddonRef.current.fit();

                const applied = {
                    cols: xtermRef.current.cols,
                    rows: xtermRef.current.rows,
                };

                if (lastResize && lastResize.cols === applied.cols && lastResize.rows === applied.rows) {
                    return;
                }

                lastResizeRef.current = applied;

                BladeDispatcher.terminal({
                    type: "Resize",
                    payload: { id, rows: applied.rows, cols: applied.cols }
                }).catch(e => console.error("Resize failed", e));
            } catch (e) {
                console.error("Resize logic error", e);
            }
        };

        const scheduleResize = () => {
            recordDebugPerf(`Terminal.scheduleResize.${id}`);
            if (resizeFrameRef.current !== null) {
                return;
            }

            resizeFrameRef.current = requestAnimationFrame(() => {
                resizeFrameRef.current = null;
                recordDebugPerf(`Terminal.resizeFrame.${id}`);
                syncTerminalSize();
            });
        };

        // Robust Fit Strategy:
        // PTY spawning needs dimensions. We must ensure the terminal has size before fitting.
        // We poll for a short period until we get valid dimensions.
        let fitAttempts = 0;
        fitIntervalRef.current = setInterval(() => {
            recordDebugPerf(`Terminal.fitInterval.${id}`);
            fitAttempts++;
            try {
                const dims = fitAddon.proposeDimensions();
                if (dims && dims.cols > 0 && dims.rows > 0) {
                    syncTerminalSize();
                    if (fitIntervalRef.current) {
                        clearInterval(fitIntervalRef.current);
                        fitIntervalRef.current = null;
                    }
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
        let disposed = false;

        if (!terminalBufferRef.current) {
            terminalBufferRef.current = new TerminalBuffer(
                (termId, data) => {
                    if (termId === id && xtermRef.current) {
                        const cleaned = sanitizeTerminalOutput(data);
                        if (cleaned) xtermRef.current.write(cleaned);
                    }
                }
            );
        }

        const unsubscribeOutput = subscribeBladeNestedEventType('Terminal', 'Output', (payload) => {
            const { id: termId, seq, data } = payload;
            if (terminalBufferRef.current) {
                terminalBufferRef.current.addOutput(termId, seq, data);
            }
        });
        const unsubscribeSpawned = subscribeBladeNestedEventType('Terminal', 'Spawned', (payload) => {
            const { id: termId, owner } = payload;
            console.debug(`[v1.1 Terminal] Spawned: id=${termId}, owner=${owner.type}`);
        });
        const unsubscribeExit = subscribeBladeNestedEventType('Terminal', 'Exit', (payload) => {
            const { id: termId, code } = payload;
            if (termId === id && xtermRef.current) {
                xtermRef.current.write(`\r\n\x1b[33mProcess exited with code ${code}\x1b[0m\r\n`);
            }
        });

        // 2. Setup backend PTY
        const initBackend = async () => {
            try {
                // Use provided cwd or fall back to workspace root
                let terminalCwd = initialCwdRef.current;
                if (!terminalCwd) {
                    const workspaceRoot = await invoke<string | null>("get_current_workspace");
                    terminalCwd = workspaceRoot || undefined;
                }

                await BladeDispatcher.terminal({
                    type: "Spawn",
                    payload: {
                        id,
                        command: initialCommandRef.current,
                        cwd: terminalCwd,
                        interactive: initialInteractiveRef.current,
                    }
                });

                if (!initialInteractiveRef.current && initialDisplayCommandRef.current) {
                    term.write(`$ ${initialDisplayCommandRef.current}\r\n`);
                }

                emit('terminal-ready', { id }).catch(console.error);
                if (id === BLADE_TERMINAL_ID) {
                    emit('blade-terminal-ready', { id }).catch(console.error);
                }

                // Initial resize after backend spawn
                setTimeout(() => {
                    scheduleResize();
                }, 50);
            } catch (err) {
                console.error("Failed to create terminal:", err);
                term.write("\r\n\x1b[31mFailed to initialize terminal backend.\x1b[0m\r\n");
            }
        };

        void initBackend();

        // 4. Send input to backend
        term.onData((data) => {
            BladeDispatcher.terminal({
                type: "Input",
                payload: { id, data }
            }).catch(console.error);
        });

        const resizeObserver = new ResizeObserver(() => {
            recordDebugPerf(`Terminal.resizeObserver.${id}`);
            scheduleResize();
        });

        resizeObserver.observe(terminalRef.current);

        return () => {
            terminalBufferRef.current?.clear(id);
            if (fitIntervalRef.current) {
                clearInterval(fitIntervalRef.current);
                fitIntervalRef.current = null;
            }
            unsubscribeOutput();
            unsubscribeSpawned();
            unsubscribeExit();

            BladeDispatcher.terminal({
                type: "Kill",
                payload: { id }
            }).catch(console.error);

            try {
                term.dispose();
            } catch (e) { console.error("Error disposing terminal", e); }

            terminalBufferRef.current?.clear(id);
            xtermRef.current = null;
            fitAddonRef.current = null;
        };
    }, [id]);

    useEffect(() => {
        if (!xtermRef.current) return;
        xtermRef.current.options.theme = getTerminalTheme();
    }, [themeId]);

    return (
        <div
            ref={terminalRef}
            className="terminal-host w-full h-full bg-(--term-bg)"
            style={{ overflow: "hidden" }}
            onContextMenu={handleContextMenu}
        />
    );

}
