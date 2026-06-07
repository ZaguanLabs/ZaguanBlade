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
            return <Check aria-hidden="true" className="w-3 h-3 text-(--accent-mention)" />;
        case 'in_progress':
            return <Loader2 aria-hidden="true" className="w-3 h-3 text-(--accent-ai) animate-spin" />;
        case 'pending':
            return <Circle aria-hidden="true" className="w-3 h-3 text-(--fg-tertiary)" />;
    }
};

const TaskPanelComponent: React.FC<TaskPanelProps> = ({ todos, isCollapsed, onToggleCollapse }) => {
    const { t } = useTranslation();
    const [isVisible, setIsVisible] = useState(false);
    const [allDoneState, setAllDoneState] = useState(false);
    const panelRef = useRef<HTMLDivElement>(null);

    const completedCount = todos.filter(t => t.status === 'completed').length;
    const totalCount = todos.length;
    const hasTodos = totalCount > 0;
    const allCompleted = totalCount > 0 && completedCount === totalCount;
    const summaryText = allDoneState
        ? t('taskPanel.allTasksCompleted')
        : t('taskPanel.completedSummary', { completed: completedCount, total: totalCount });

    // Slide-in animation on mount
    useEffect(() => {
        if (hasTodos) {
            requestAnimationFrame(() => setIsVisible(true));
        }
    }, [hasTodos]);

    // "All done" brief display state
    useEffect(() => {
        if (allCompleted && !allDoneState) {
            setAllDoneState(true);
        }
    }, [allCompleted, allDoneState]);

    if (!hasTodos) return null;

    return (
        <div
            ref={panelRef}
            className={`border-t border-(--border-subtle) bg-(--bg-surface) transition-all duration-300 ease-out ${
                isVisible ? 'opacity-100 max-h-[300px]' : 'opacity-0 max-h-0'
            } ${allDoneState ? 'border-t-(--accent-mention)/30' : ''}`}
            style={{ overflow: 'hidden' }}
        >
            <button
                type="button"
                onClick={onToggleCollapse}
                aria-expanded={!isCollapsed}
                className="w-full flex items-center gap-2 px-2.5 py-1.5 hover:bg-(--bg-surface-hover) transition-colors text-left"
            >
                <div className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-[calc(var(--panel-radius)*0.35)] border ${allDoneState ? 'border-(--accent-mention)/20 bg-[color-mix(in_srgb,var(--accent-mention)_10%,transparent)]' : 'border-(--accent-ai)/20 bg-[color-mix(in_srgb,var(--accent-ai)_10%,transparent)]'}`}>
                    <Zap className={`h-3 w-3 ${allDoneState ? 'text-(--accent-mention)' : 'text-(--accent-ai)'}`} />
                </div>
                <span className={`flex-1 truncate text-[10px] font-medium uppercase tracking-[0.12em] ${
                    allDoneState ? 'text-(--accent-mention)' : 'text-(--fg-secondary)'
                }`}>
                    {summaryText}
                </span>
                <span className="shrink-0 text-[10px] text-(--fg-tertiary)">
                    {t('taskPanel.itemsCount', { count: totalCount })}
                </span>
                {isCollapsed ? (
                    <ChevronRight className="w-3 h-3 text-(--fg-tertiary) shrink-0" />
                ) : (
                    <ChevronDown className="w-3 h-3 text-(--fg-tertiary) shrink-0" />
                )}
            </button>

            <div
                className={`transition-all duration-200 ease-out ${
                    isCollapsed ? 'max-h-0 opacity-0' : 'max-h-[240px] opacity-100'
                }`}
                style={{ overflow: isCollapsed ? 'hidden' : 'auto' }}
            >
                <div role="list" className="px-2.5 pb-2 space-y-0.5">
                    {todos.map((todo, index) => {
                        const text = todo.status === 'in_progress' ? todo.activeForm : todo.content;
                        const statusLabel = todo.status === 'completed'
                            ? t('taskPanel.statusCompleted')
                            : todo.status === 'in_progress'
                            ? t('taskPanel.statusInProgress')
                            : t('taskPanel.statusPending');
                        return (
                            <div
                                key={index}
                                role="listitem"
                                aria-label={t('taskPanel.taskRowLabel', { index: index + 1, status: statusLabel, text })}
                                className={`flex items-center gap-1.5 border-l-2 px-1.5 py-1 text-[10px] leading-snug transition-colors ${
                                    todo.status === 'completed'
                                        ? 'border-(--border-subtle) text-(--fg-tertiary) line-through'
                                        : todo.status === 'in_progress'
                                        ? 'border-(--accent-ai) text-(--fg-primary) font-medium'
                                        : 'border-(--border-subtle) text-(--fg-tertiary)'
                                }`}
                            >
                                <StatusIcon status={todo.status} />
                                <span className="w-4 shrink-0 text-right font-mono text-[9px] text-(--fg-tertiary)">
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
