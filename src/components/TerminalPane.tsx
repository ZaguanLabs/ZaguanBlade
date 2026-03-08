"use client";

import React, { useState, useEffect, forwardRef, useImperativeHandle, useCallback } from "react";
import Terminal from "./Terminal";
import { Plus, X, Terminal as TerminalIcon } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { BladeDispatcher } from "../services/blade";

interface TerminalTab {
    id: string;
    title: string;
    cwd?: string;
    command?: string;
    transient?: boolean;
}

interface PersistedTerminalTab {
    id: string;
    title: string;
    cwd?: string;
}

export interface TerminalPaneHandle {
    getTerminalState: () => { terminals: PersistedTerminalTab[]; activeId: string };
    restoreTerminals: (terminals: PersistedTerminalTab[], activeId?: string) => void;
}

export const TerminalPane = forwardRef<TerminalPaneHandle>((_, ref) => {
    const [terminals, setTerminals] = useState<TerminalTab[]>([]);
    const [activeId, setActiveId] = useState<string>("");

    const getTitleFromCwd = (path?: string, fallback = "Terminal") => {
        if (!path) return fallback;
        const normalized = path.replace(/[\\/]+$/, "");
        if (!normalized) return fallback;
        const parts = normalized.split(/[\\/]/);
        return parts[parts.length - 1] || fallback;
    };

    useImperativeHandle(ref, () => ({
        getTerminalState: () => {
            const persistedTerminals = terminals
                .filter((term) => !term.transient)
                .map(({ id, title, cwd }) => ({ id, title, cwd }));

            return {
                terminals: persistedTerminals,
                activeId: persistedTerminals.some((term) => term.id === activeId) ? activeId : (persistedTerminals[0]?.id || ""),
            };
        },
        restoreTerminals: (restoredTerminals: PersistedTerminalTab[], restoredActiveId?: string) => {
            if (restoredTerminals.length > 0) {
                const normalized = restoredTerminals.map((term) =>
                    term.cwd ? { ...term, title: getTitleFromCwd(term.cwd, term.title) } : term
                );
                const nextActive = restoredActiveId && normalized.some(term => term.id === restoredActiveId)
                    ? restoredActiveId
                    : normalized[0].id;
                setTerminals(normalized);
                setActiveId(nextActive);
            } else {
                setTerminals([]);
                setActiveId("");
            }
        },
    }), [terminals, activeId]);

    // Set initial terminal cwd/title to workspace root if available
    useEffect(() => {
        let isMounted = true;
        const initWorkspace = async () => {
            try {
                const workspaceRoot = await invoke<string | null>("get_current_workspace");
                if (!isMounted || !workspaceRoot) return;
                setTerminals(prev =>
                    prev.map(term =>
                        term.cwd
                            ? { ...term, title: getTitleFromCwd(term.cwd, term.title) }
                            : { ...term, cwd: workspaceRoot, title: getTitleFromCwd(workspaceRoot, term.title) }
                    )
                );
            } catch {
                // ignore
            }
        };

        initWorkspace();
        return () => {
            isMounted = false;
        };
    }, []);

    // Listen for open-terminal events from other components (e.g., File Explorer)
    useEffect(() => {
        const unlisten = listen<{
            path?: string;
            cwd?: string;
            id?: string;
            title?: string;
            command?: string;
            focus?: boolean;
            transient?: boolean;
        }>('open-terminal', (event) => {
            const cwd = event.payload.cwd ?? event.payload.path;
            const id = event.payload.id ?? `term-${Date.now()}`;
            const title = event.payload.title ?? getTitleFromCwd(cwd, 'Terminal');
            const focus = event.payload.focus ?? true;

            setTerminals(prev => {
                const existingIndex = prev.findIndex((term) => term.id === id);
                const nextTab: TerminalTab = {
                    id,
                    title,
                    cwd,
                    command: event.payload.command,
                    transient: event.payload.transient,
                };

                if (existingIndex >= 0) {
                    const next = [...prev];
                    next[existingIndex] = {
                        ...next[existingIndex],
                        ...nextTab,
                        title: nextTab.title || next[existingIndex].title,
                    };
                    return next;
                }

                return [...prev, nextTab];
            });

            if (focus) {
                setActiveId(id);
            }

            console.debug('[TerminalPane] Opening terminal:', { id, cwd, title, hasCommand: !!event.payload.command });
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, []);

    // Update terminal titles when backend reports cwd changes
    useEffect(() => {
        const unlisten = listen<{ id: string; cwd: string }>('terminal-cwd-changed', (event) => {
            const { id, cwd } = event.payload;
            setTerminals(prev =>
                prev.map(term => {
                    if (term.id !== id) return term;
                    return { ...term, cwd, title: getTitleFromCwd(cwd, term.title) };
                })
            );
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, []);

    const addTerminal = async () => {
        const newId = `term-${Date.now()}`;
        let terminalCwd: string | undefined;
        try {
            const workspaceRoot = await invoke<string | null>("get_current_workspace");
            terminalCwd = workspaceRoot || undefined;
        } catch {
            terminalCwd = undefined;
        }
        const newTab = {
            id: newId,
            title: getTitleFromCwd(terminalCwd, "Terminal"),
            cwd: terminalCwd,
        };
        setTerminals(prev => [...prev, newTab]);
        setActiveId(newId);
    };

    const closeTerminal = (e: React.MouseEvent, id: string) => {
        e.stopPropagation();
        const newTerminals = terminals.filter((t) => t.id !== id);
        setTerminals(newTerminals);

        if (activeId === id) {
            setActiveId(newTerminals[newTerminals.length - 1]?.id || "");
        }

        BladeDispatcher.terminal({
            type: "Kill",
            payload: { id }
        }).catch(console.error);
    };

    const suppressContextMenu = useCallback((e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();
    }, []);

    return (
        <div className="h-full min-w-0 overflow-hidden flex flex-row bg-[#1e1e1e]">
            {/* Terminal Area */}
            <div className="flex-1 min-w-0 relative overflow-hidden">
                {terminals.map((term) => (
                    <div
                        key={term.id}
                        className="absolute inset-0 w-full h-full"
                        style={{
                            visibility: term.id === activeId ? "visible" : "hidden",
                            zIndex: term.id === activeId ? 10 : 0
                        }}
                    >
                        <Terminal id={term.id} cwd={term.cwd} command={term.command} />
                    </div>
                ))}
            </div>

            {/* Tabs Sidebar (Right side like VSCode default or requested image) */}
            <div
                className="w-48 bg-[var(--bg-app)] border-l border-[var(--border-default)] shadow-[var(--shadow-md)] flex flex-col"
                onContextMenu={suppressContextMenu}
            >
                <div className="p-2 text-xs font-semibold text-[var(--fg-tertiary)] uppercase tracking-wider flex items-center justify-between">
                    <span>Terminals</span>
                    <button onClick={addTerminal} className="hover:text-[var(--fg-primary)] transition-colors">
                        <Plus className="w-4 h-4" />
                    </button>
                </div>

                <div className="flex-1 overflow-y-auto">
                    {terminals.map((term) => (
                        <div
                            key={term.id}
                            onClick={() => setActiveId(term.id)}
                            className={`
                    group flex items-center justify-between px-3 py-2 cursor-pointer text-sm border-l-2
                    ${activeId === term.id
                                    ? "bg-[#37373d] border-blue-500 text-white"
                                    : "border-transparent text-zinc-400 hover:bg-[#2a2d2e] hover:text-zinc-200"
                                }
                `}
                        >
                            <div className="flex items-center gap-2 truncate">
                                <TerminalIcon className="w-3.5 h-3.5 opacity-70" />
                                <span className="truncate">{term.title}</span>
                            </div>
                            {terminals.length > 1 && (
                                <button
                                    onClick={(e) => closeTerminal(e, term.id)}
                                    className="opacity-0 group-hover:opacity-100 hover:bg-zinc-600 rounded p-0.5"
                                >
                                    <X className="w-3 h-3" />
                                </button>
                            )}
                        </div>
                    ))}
                </div>
            </div>
        </div>
    );
});
