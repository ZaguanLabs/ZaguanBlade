import React from 'react';
import { useTranslation } from 'react-i18next';
import { CheckCircle2, Circle, Loader2 } from 'lucide-react';
import type { TodoItem } from '../../types/events';

export const TaskStrip: React.FC<{ todos: TodoItem[] }> = ({ todos }) => {
    const { t } = useTranslation();

    if (todos.length === 0) {
        return null;
    }

    return (
        <div className="shrink-0 px-2 pt-1.5">
            <div className="mb-1.5 flex items-center gap-2 text-[9px] font-semibold uppercase tracking-[0.16em] text-(--fg-tertiary)">
                <span className="text-(--accent-ai)">Plan</span>
                <span className="rounded-full border border-(--border-subtle) px-1.5 py-0.5 text-[9px] leading-none text-(--fg-tertiary)">
                    {todos.length}
                </span>
            </div>
            <div role="list" className="space-y-1">
                {todos.map((todo, index) => {
                    const Icon = todo.status === 'completed' ? CheckCircle2 : todo.status === 'in_progress' ? Loader2 : Circle;
                    const statusLabel = todo.status === 'completed'
                        ? t('taskPanel.statusCompleted')
                        : todo.status === 'in_progress'
                        ? t('taskPanel.statusInProgress')
                        : t('taskPanel.statusPending');
                    return (
                        <div key={`${todo.content}:${index}`} role="listitem" aria-label={t('taskPanel.taskRowLabel', { index: index + 1, status: statusLabel, text: todo.content })} className="flex items-center gap-2 text-[11px] text-(--fg-secondary)">
                            <Icon aria-hidden="true" className={`h-3.5 w-3.5 ${todo.status === 'in_progress' ? 'animate-spin text-(--accent-ai)' : todo.status === 'completed' ? 'text-(--accent-mention)' : 'text-(--fg-tertiary)'}`} />
                            <span className={todo.status === 'completed' ? 'line-through opacity-70' : ''}>{todo.content}</span>
                        </div>
                    );
                })}
            </div>
        </div>
    );
};
