'use client';
import React, { useState, useRef, useEffect } from 'react';
import { ModelInfo } from '../types/chat';
import { ChevronDown, Check, Box, Search, Cpu, Sparkles, BrainCircuit } from 'lucide-react';

interface ModelSelectorProps {
    models: ModelInfo[];
    selectedId: string;
    onSelect: (id: string) => void;
    disabled?: boolean;
}

export const ModelSelector: React.FC<ModelSelectorProps> = ({ models, selectedId, onSelect, disabled }) => {
    const [isOpen, setIsOpen] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const selectedModel = models.find(m => m.id === selectedId) || null;

    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
                setIsOpen(false);
            }
        };

        if (isOpen) {
            document.addEventListener('mousedown', handleClickOutside);
        }
        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
        };
    }, [isOpen]);

    const getModelIcon = (id: string) => {
        const lower = id.toLowerCase();
        if (lower.includes('gpt')) return <Sparkles className="w-3.5 h-3.5 text-green-400" />;
        if (lower.includes('claude')) return <BrainCircuit className="w-3.5 h-3.5 text-orange-400" />;
        if (lower.includes('gemini')) return <Cpu className="w-3.5 h-3.5 text-blue-400" />;
        return <Box className="w-3.5 h-3.5 text-zinc-400" />;
    };

    return (
        <div className="relative w-full" ref={containerRef}>
            <button
                type="button"
                onClick={() => !disabled && setIsOpen(!isOpen)}
                disabled={disabled}
                className={`
                    w-full flex items-center justify-between px-3 py-1.5 
                    bg-[var(--bg-surface)] hover:bg-[var(--bg-surface-hover)]
                    border border-[var(--border-subtle)] hover:border-[var(--border-hover)]
                    rounded transition-all duration-200 group
                    ${isOpen ? 'ring-1 ring-[var(--accent-primary)]/50 border-[var(--accent-primary)]/50' : ''}
                    ${disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}
                `}
            >
                <div className="flex items-center gap-2 overflow-hidden">
                    <div className="shrink-0 p-0.5 rounded bg-[var(--bg-app)]/50 border border-[var(--border-subtle)]">
                        {selectedModel ? getModelIcon(selectedModel.id) : <Box className="w-3 h-3" />}
                    </div>
                    <div className="flex flex-col items-start min-w-0">
                        <span className="text-[11px] font-medium text-[var(--fg-secondary)] truncate w-full text-left">
                            {selectedModel?.name || 'Select Model'}
                        </span>
                    </div>
                </div>
                <ChevronDown className={`w-3 h-3 text-[var(--fg-tertiary)] transition-transform duration-200 ${isOpen ? 'rotate-180' : ''}`} />
            </button>

            {isOpen && (
                <div className="absolute top-full left-0 right-0 mt-1.5 py-1 bg-[var(--bg-panel)] border border-[var(--border-subtle)] rounded shadow-xl z-50 max-h-[300px] overflow-y-auto flex flex-col gap-0.5 animate-in fade-in zoom-in-95 duration-100 origin-top">
                    {models.length === 0 && (
                        <div className="px-3 py-2 text-xs text-[var(--fg-tertiary)] text-center italic">
                            No models available
                        </div>
                    )}
                    {models.map(model => {
                        const isSelected = model.id === selectedId;
                        return (
                            <button
                                key={model.id}
                                onClick={() => {
                                    onSelect(model.id);
                                    setIsOpen(false);
                                }}
                                className={`
                                    flex items-center gap-3 px-3 py-2 mx-1 rounded-sm text-left
                                    transition-colors duration-150
                                    ${isSelected
                                        ? 'bg-[var(--accent-primary)]/10 text-[var(--fg-primary)]'
                                        : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)]'
                                    }
                                `}
                            >
                                <div className="shrink-0 mt-0.5">
                                    {getModelIcon(model.id)}
                                </div>
                                <div className="flex flex-col min-w-0 flex-1">
                                    <span className="text-xs font-medium truncate">
                                        {model.name}
                                    </span>
                                    {model.description && (
                                        <span className="text-[10px] text-[var(--fg-tertiary)] truncate opacity-80">
                                            {model.description}
                                        </span>
                                    )}
                                </div>
                                {isSelected && <Check className="w-3.5 h-3.5 text-[var(--accent-primary)] shrink-0" />}
                            </button>
                        );
                    })}
                </div>
            )}
        </div>
    );
};
