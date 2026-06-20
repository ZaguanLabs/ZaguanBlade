import React from 'react';
import { useTranslation } from 'react-i18next';
import { CheckCircle2, Circle, Loader2 } from 'lucide-react';
import type { TodoItem } from '../../types/events';
import { StatusStripFrame } from './StatusStripFrame';

export const TaskStrip: React.FC<{ todos: TodoItem[] }> = ({ todos }) => {
    const { t } = useTranslation();

    if (todos.length === 0) {
        return null;
    }

    return (
        <StatusStripFrame label="Plan" count={todos.length} tone="ai" outerBackground={false} bottomCorners="square">
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
        </StatusStripFrame>
    );
};
