"use client";
import React, { useState } from "react";
import { X } from "lucide-react";

interface Tab {
    path: string;
    filename: string;
}

interface EditorTabsProps {
    tabs: Tab[];
    activeTab: string | null;
    onTabClick: (path: string) => void;
    onTabClose: (path: string) => void;
}

export const EditorTabs: React.FC<EditorTabsProps> = ({ tabs, activeTab, onTabClick, onTabClose }) => {
    return (
        <div className="flex items-center bg-[#252526] border-b border-[#3c3c3c] overflow-x-auto scrollbar-thin scrollbar-thumb-zinc-700">
            {tabs.map((tab) => {
                const isActive = tab.path === activeTab;
                return (
                    <div
                        key={tab.path}
                        onClick={() => onTabClick(tab.path)}
                        className={`
                            group flex items-center gap-2 px-4 py-2 text-sm font-mono cursor-pointer
                            border-r border-[#3c3c3c] min-w-[120px] max-w-[200px]
                            transition-colors relative
                            ${isActive
                                ? 'bg-[#1e1e1e] text-white'
                                : 'bg-[#2d2d2d] text-zinc-400 hover:bg-[#323232] hover:text-zinc-200'
                            }
                        `}
                    >
                        {isActive && (
                            <div className="absolute top-0 left-0 right-0 h-[2px] bg-emerald-500" />
                        )}
                        <span className="truncate flex-1">{tab.filename}</span>
                        <button
                            onClick={(e) => {
                                e.stopPropagation();
                                onTabClose(tab.path);
                            }}
                            className="opacity-0 group-hover:opacity-100 hover:bg-zinc-600 rounded p-0.5 transition-opacity"
                        >
                            <X className="w-3 h-3" />
                        </button>
                    </div>
                );
            })}
        </div>
    );
};
