import React from 'react';
import { CheckCircle2, Circle, Loader2 } from 'lucide-react';
import type { TodoItem } from '../../types/events';
import { StatusStripFrame } from './StatusStripFrame';

export const TaskStrip: React.FC<{ todos: TodoItem[] }> = ({ todos }) => {
    if (todos.length === 0) {
        return null;
    }

    return (
        <StatusStripFrame label="Plan" count={todos.length} tone="ai">
            {todos.map((todo, index) => {
                const Icon = todo.status === 'completed' ? CheckCircle2 : todo.status === 'in_progress' ? Loader2 : Circle;
                return (
                    <div key={`${todo.content}:${index}`} className="flex items-center gap-2 text-[11px] text-(--fg-secondary)">
                        <Icon className={`h-3.5 w-3.5 ${todo.status === 'in_progress' ? 'animate-spin text-(--accent-ai)' : todo.status === 'completed' ? 'text-(--accent-mention)' : 'text-(--fg-tertiary)'}`} />
                        <span className={todo.status === 'completed' ? 'line-through opacity-70' : ''}>{todo.content}</span>
                    </div>
                );
            })}
        </StatusStripFrame>
    );
};
