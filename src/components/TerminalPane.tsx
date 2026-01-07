"use client";

import React, { useState } from "react";
import Terminal from "./Terminal";
import { Plus, X, Terminal as TerminalIcon } from "lucide-react";

interface TerminalTab {
    id: string;
    title: string;
}

export const TerminalPane: React.FC = () => {
    const [terminals, setTerminals] = useState<TerminalTab[]>([
        { id: "term-1", title: "Terminal 1" },
    ]);
    const [activeId, setActiveId] = useState<string>("term-1");

    const addTerminal = () => {
        const newId = `term-${Date.now()}`;
        const newTab = { id: newId, title: "zsh" };
        setTerminals([...terminals, newTab]);
        setActiveId(newId);
    };

    const closeTerminal = (e: React.MouseEvent, id: string) => {
        e.stopPropagation();
        if (terminals.length === 1) return; // Prevent closing last terminal for now?

        const newTerminals = terminals.filter((t) => t.id !== id);
        setTerminals(newTerminals);

        if (activeId === id) {
            setActiveId(newTerminals[newTerminals.length - 1].id);
        }

        // TODO: Send kill command to backend for this ID
    };

    return (
        <div className="h-full flex flex-row bg-[#1e1e1e]">
            {/* Terminal Area */}
            <div className="flex-1 relative overflow-hidden">
                {terminals.map((term) => (
                    <div
                        key={term.id}
                        className="absolute inset-0 w-full h-full"
                        style={{
                            visibility: term.id === activeId ? "visible" : "hidden",
                            zIndex: term.id === activeId ? 10 : 0
                        }}
                    >
                        <Terminal id={term.id} />
                    </div>
                ))}
            </div>

            {/* Tabs Sidebar (Right side like VSCode default or requested image) */}
            <div className="w-48 bg-[#252526] border-l border-[#3c3c3c] flex flex-col">
                <div className="p-2 text-xs font-semibold text-zinc-400 uppercase tracking-wider flex items-center justify-between">
                    <span>Terminals</span>
                    <button onClick={addTerminal} className="hover:text-white transition-colors">
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
};
