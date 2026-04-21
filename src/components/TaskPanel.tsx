import React, { useState, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
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
    const { t } = useTranslation();
    const [isVisible, setIsVisible] = useState(false);
    const [allDoneState, setAllDoneState] = useState(false);
    const panelRef = useRef<HTMLDivElement>(null);

    const completedCount = todos.filter(t => t.status === 'completed').length;
    const totalCount = todos.length;
    const allCompleted = totalCount > 0 && completedCount === totalCount;
    const summaryText = allDoneState
        ? t('taskPanel.allTasksCompleted')
        : t('taskPanel.completedSummary', { completed: completedCount, total: totalCount });

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
            className={`border-t border-[var(--border-subtle)] bg-[var(--bg-surface)] transition-all duration-300 ease-out ${
                isVisible ? 'opacity-100 max-h-[300px]' : 'opacity-0 max-h-0'
            } ${allDoneState ? 'border-t-emerald-500/30' : ''}`}
            style={{ overflow: 'hidden' }}
        >
            <button
                onClick={onToggleCollapse}
                className="w-full flex items-center gap-2 px-2.5 py-1.5 hover:bg-[var(--bg-surface-hover)] transition-colors text-left"
            >
                <div className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-md border ${allDoneState ? 'border-emerald-500/20 bg-emerald-500/10' : 'border-indigo-500/20 bg-indigo-500/10'}`}>
                    <Zap className={`h-3 w-3 ${allDoneState ? 'text-emerald-400' : 'text-indigo-400'}`} />
                </div>
                <span className={`flex-1 truncate text-[10px] font-medium uppercase tracking-[0.12em] ${
                    allDoneState ? 'text-emerald-400' : 'text-[var(--fg-secondary)]'
                }`}>
                    {summaryText}
                </span>
                <span className="shrink-0 text-[10px] text-zinc-500">
                    {t('taskPanel.itemsCount', { count: totalCount })}
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
                <div className="px-2.5 pb-2 space-y-0.5">
                    {todos.map((todo, index) => {
                        const text = todo.status === 'in_progress' ? todo.activeForm : todo.content;
                        return (
                            <div
                                key={index}
                                className={`flex items-center gap-1.5 border-l-2 px-1.5 py-1 text-[10px] leading-snug transition-colors ${
                                    todo.status === 'completed'
                                        ? 'border-zinc-800 text-zinc-600 line-through'
                                        : todo.status === 'in_progress'
                                        ? 'border-indigo-400 text-[var(--fg-primary)] font-medium'
                                        : 'border-zinc-800 text-zinc-500'
                                }`}
                            >
                                <StatusIcon status={todo.status} />
                                <span className="w-4 shrink-0 text-right font-mono text-[9px] text-zinc-600">
                                    {index + 1}.
                                </span>
                                <span className="min-w-0 flex-1 truncate">{text}</span>
                            </div>
                        );
                    })}
                </div>
            </div>
        </div>
    );
};

export const TaskPanel = React.memo(TaskPanelComponent);
