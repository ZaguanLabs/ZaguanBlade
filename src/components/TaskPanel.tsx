import React, { useState, useEffect, useRef } from 'react';
import { Check, Circle, Loader2, ChevronDown, ChevronRight, Zap } from 'lucide-react';
import type { TodoItem } from '../types/events';

interface TaskPanelProps {
    todos: TodoItem[];
    isCollapsed: boolean;
    onToggleCollapse: () => void;
}

const StatusIcon: React.FC<{ status: TodoItem['status'] }> = ({ status }) => {
    switch (status) {
        case 'completed':
            return <Check className="w-3 h-3 text-emerald-400" />;
        case 'in_progress':
            return <Loader2 className="w-3 h-3 text-indigo-400 animate-spin" />;
        case 'pending':
            return <Circle className="w-3 h-3 text-zinc-600" />;
    }
};

const TaskPanelComponent: React.FC<TaskPanelProps> = ({ todos, isCollapsed, onToggleCollapse }) => {
    const [isVisible, setIsVisible] = useState(false);
    const [allDoneState, setAllDoneState] = useState(false);
    const panelRef = useRef<HTMLDivElement>(null);

    const completedCount = todos.filter(t => t.status === 'completed').length;
    const totalCount = todos.length;
    const allCompleted = totalCount > 0 && completedCount === totalCount;

    // Slide-in animation on mount
    useEffect(() => {
        if (todos.length > 0) {
            requestAnimationFrame(() => setIsVisible(true));
        }
    }, [todos.length > 0]);

    // "All done" brief display state
    useEffect(() => {
        if (allCompleted && !allDoneState) {
            setAllDoneState(true);
        }
    }, [allCompleted, allDoneState]);

    if (todos.length === 0) return null;

    return (
        <div
            ref={panelRef}
            className={`border-t border-[var(--border-subtle)] bg-[var(--bg-surface)]/80 backdrop-blur-sm transition-all duration-300 ease-out ${
                isVisible ? 'opacity-100 max-h-[300px]' : 'opacity-0 max-h-0'
            } ${allDoneState ? 'border-t-emerald-500/30' : ''}`}
            style={{ overflow: 'hidden' }}
        >
            <button
                onClick={onToggleCollapse}
                className="w-full flex items-center gap-3 px-3 py-2.5 hover:bg-[var(--bg-surface-hover)] transition-colors text-left"
            >
                <div className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-xl border ${allDoneState ? 'border-emerald-500/20 bg-emerald-500/10' : 'border-indigo-500/20 bg-indigo-500/10'}`}>
                    <Zap className={`h-3.5 w-3.5 ${allDoneState ? 'text-emerald-400' : 'text-indigo-400'}`} />
                </div>
                <span className={`flex-1 text-[11px] font-semibold uppercase tracking-[0.16em] ${
                    allDoneState ? 'text-emerald-400' : 'text-[var(--fg-secondary)]'
                }`}>
                    {allDoneState
                        ? `All tasks completed ✓`
                        : `${completedCount} of ${totalCount} tasks completed`
                    }
                </span>
                {isCollapsed ? (
                    <ChevronRight className="w-3 h-3 text-zinc-500 shrink-0" />
                ) : (
                    <ChevronDown className="w-3 h-3 text-zinc-500 shrink-0" />
                )}
            </button>

            <div
                className={`transition-all duration-200 ease-out ${
                    isCollapsed ? 'max-h-0 opacity-0' : 'max-h-[240px] opacity-100'
                }`}
                style={{ overflow: isCollapsed ? 'hidden' : 'auto' }}
            >
                <div className="px-3 pb-3 space-y-1">
                    {todos.map((todo, index) => {
                        const text = todo.status === 'in_progress' ? todo.activeForm : todo.content;
                        return (
                            <div
                                key={index}
                                className={`flex items-center gap-2 rounded-xl border px-2.5 py-2 text-[11px] leading-tight transition-all duration-200 ${
                                    todo.status === 'completed'
                                        ? 'border-zinc-800/60 bg-zinc-950/30 text-zinc-600 line-through'
                                        : todo.status === 'in_progress'
                                        ? 'border-indigo-500/15 bg-indigo-500/5 text-[var(--fg-primary)] font-medium'
                                        : 'border-zinc-800/60 bg-zinc-950/20 text-zinc-500'
                                }`}
                            >
                                <StatusIcon status={todo.status} />
                                <span className="w-4 shrink-0 text-right font-mono text-[10px] text-zinc-600">
                                    {index + 1}.
                                </span>
                                <span className="truncate">{text}</span>
                            </div>
                        );
                    })}
                </div>
            </div>
        </div>
    );
};

export const TaskPanel = React.memo(TaskPanelComponent);
