import React from 'react';
import { CheckCircle2, Circle, Loader2 } from 'lucide-react';
import type { TodoItem } from '../../types/events';

export const TaskStrip: React.FC<{ todos: TodoItem[] }> = ({ todos }) => {
    if (todos.length === 0) {
        return null;
    }

    return (
        <div className="shrink-0 border-t border-(--border-subtle) bg-(--bg-app) px-3 py-2">
            <div className="space-y-1 rounded-lg border border-(--border-subtle) bg-(--bg-surface)/60 px-3 py-2">
                {todos.map((todo, index) => {
                    const Icon = todo.status === 'completed' ? CheckCircle2 : todo.status === 'in_progress' ? Loader2 : Circle;
                    return (
                        <div key={`${todo.content}:${index}`} className="flex items-center gap-2 text-[11px] text-(--fg-secondary)">
                            <Icon className={`h-3.5 w-3.5 ${todo.status === 'in_progress' ? 'animate-spin text-(--accent-primary)' : ''}`} />
                            <span className={todo.status === 'completed' ? 'line-through opacity-70' : ''}>{todo.content}</span>
                        </div>
                    );
                })}
            </div>
        </div>
    );
};
