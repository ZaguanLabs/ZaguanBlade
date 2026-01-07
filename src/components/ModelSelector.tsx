'use client';
import React from 'react';
import { ModelInfo } from '../types/chat';
import { ChevronDown } from 'lucide-react';

interface ModelSelectorProps {
    models: ModelInfo[];
    selectedId: string;
    onSelect: (id: string) => void;
    disabled?: boolean;
}

export const ModelSelector: React.FC<ModelSelectorProps> = ({ models, selectedId, onSelect, disabled }) => {
    return (
        <div className="relative group">
            <select
                value={selectedId}
                onChange={(e) => onSelect(e.target.value)}
                disabled={disabled}
                className="appearance-none bg-zinc-900 border border-zinc-800 text-zinc-400 text-xs py-1 pl-2 pr-6 rounded-sm focus:outline-none focus:border-zinc-700 hover:border-zinc-700 transition-colors w-32 truncate cursor-pointer"
            >
                {models.map(m => (
                    <option key={m.id} value={m.id}>
                        {m.name}
                    </option>
                ))}
            </select>
            <ChevronDown className="w-3 h-3 text-zinc-600 absolute right-2 top-1.5 pointer-events-none group-hover:text-zinc-500" />
        </div>
    );
};
