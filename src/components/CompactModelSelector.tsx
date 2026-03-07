import React, { useState, useRef, useEffect, useMemo } from 'react';
import { ModelInfo } from '../types/chat';
import { ChevronDown, Check, Box, Cpu, Sparkles, BrainCircuit } from 'lucide-react';

interface CompactModelSelectorProps {
    models: ModelInfo[];
    selectedId: string;
    onSelect: (id: string) => void;
    disabled?: boolean;
}

const CompactModelSelectorComponent: React.FC<CompactModelSelectorProps> = ({ models, selectedId, onSelect, disabled }) => {
    const [isOpen, setIsOpen] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const dropdownRef = useRef<HTMLDivElement>(null);
    const selectedModel = useMemo(() => models.find(m => m.id === selectedId) || null, [models, selectedId]);
    const cloudModels = useMemo(
        () => models.filter(model => model.provider !== 'ollama' && model.provider !== 'openai-compat'),
        [models]
    );
    const ollamaModels = useMemo(
        () => models.filter(model => model.provider === 'ollama'),
        [models]
    );
    const openaiCompatModels = useMemo(
        () => models.filter(model => model.provider === 'openai-compat'),
        [models]
    );

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

    // Scroll to selected model when dropdown opens
    useEffect(() => {
        if (isOpen && dropdownRef.current && selectedId) {
            requestAnimationFrame(() => {
                const container = dropdownRef.current;
                const selectedButton = container?.querySelector<HTMLElement>(`[data-model-id="${selectedId}"]`);
                if (container && selectedButton) {
                    const itemTop = selectedButton.offsetTop;
                    const itemBottom = itemTop + selectedButton.offsetHeight;
                    const viewportTop = container.scrollTop;
                    const viewportBottom = viewportTop + container.clientHeight;

                    if (itemTop < viewportTop) {
                        container.scrollTop = itemTop;
                    } else if (itemBottom > viewportBottom) {
                        container.scrollTop = itemBottom - container.clientHeight;
                    }
                }
            });
        }
    }, [isOpen, selectedId]);

    const getModelIcon = (id: string) => {
        const lower = id.toLowerCase();
        if (lower.includes('gpt')) return <Sparkles className="w-3 h-3 text-green-400" />;
        if (lower.includes('claude')) return <BrainCircuit className="w-3 h-3 text-orange-400" />;
        if (lower.includes('gemini')) return <Cpu className="w-3 h-3 text-blue-400" />;
        return <Box className="w-3 h-3 text-zinc-400" />;
    };

    return (
        <div className="relative w-full" ref={containerRef}>
            <button
                type="button"
                onClick={() => !disabled && setIsOpen(!isOpen)}
                disabled={disabled}
                className={`
                    w-full flex items-center justify-between gap-1.5 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-app)] px-2 py-1
                    transition-colors duration-150
                    ${disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}
                `}
            >
                <div className="flex items-center gap-1 overflow-hidden">
                    <div className="flex h-4.5 w-4.5 shrink-0 items-center justify-center rounded-sm border border-[var(--border-subtle)] bg-[var(--bg-surface)]">
                        {selectedModel ? getModelIcon(selectedModel.id) : <Box className="w-3 h-3" />}
                    </div>
                    <span className="truncate text-[10px] font-medium text-[var(--fg-secondary)]">
                        {selectedModel?.name || 'Select Model'}
                    </span>
                </div>
                <ChevronDown className={`h-2.5 w-2.5 text-[var(--fg-tertiary)] transition-transform duration-200 ${isOpen ? 'rotate-180' : ''}`} />
            </button>

            {isOpen && (
                <div
                    ref={dropdownRef}
                    onWheel={(event) => event.stopPropagation()}
                    className="fixed bottom-[52px] right-2 z-110 flex max-h-[240px] w-64 flex-col gap-0.5 overflow-y-auto overscroll-contain rounded-lg border border-[var(--border-focus)] bg-[var(--bg-surface)] py-1 shadow-[0_20px_52px_rgba(0,0,0,0.35)] animate-in fade-in zoom-in-95 duration-100 origin-bottom-right"
                    style={{
                        boxShadow: '0 8px 30px rgba(0, 0, 0, 0.4), 0 0 1px rgba(255, 255, 255, 0.1)',
                        overscrollBehavior: 'contain',
                    }}
                >
                    {models.length === 0 && (
                        <div className="px-2 py-1.5 text-center text-[9px] italic text-[var(--fg-tertiary)]">
                            No models available
                        </div>
                    )}
                    {cloudModels.map(model => {
                        const isSelected = model.id === selectedId;
                        return (
                            <button
                                key={model.id}
                                data-model-id={model.id}
                                onClick={() => {
                                    onSelect(model.id);
                                    setIsOpen(false);
                                }}
                                className={`
                                    mx-1 flex items-center gap-1.5 rounded-md px-2 py-1.5 text-left
                                    transition-colors duration-150
                                    ${isSelected
                                        ? 'bg-[var(--accent-primary)]/10 text-[var(--fg-primary)]'
                                        : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)]'
                                    }
                                `}
                            >
                                <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                                    {getModelIcon(model.id)}
                                </div>
                                <div className="flex flex-col min-w-0 flex-1">
                                    <span className="truncate text-[10px] font-medium">
                                        {model.name}
                                    </span>
                                    {model.description && (
                                        <span className="text-[9px] text-[var(--fg-tertiary)] truncate opacity-80">
                                            {model.description}
                                        </span>
                                    )}
                                </div>
                                {isSelected && <Check className="w-2.5 h-2.5 text-[var(--accent-primary)] shrink-0" />}
                            </button>
                        );
                    })}
                    {ollamaModels.length > 0 && (
                        <div className="border-t border-[var(--border-subtle)] px-2 pt-2 text-[8px] font-semibold uppercase tracking-[0.16em] text-[var(--fg-tertiary)]">
                            Ollama
                        </div>
                    )}
                    {ollamaModels.map(model => {
                        const isSelected = model.id === selectedId;
                        return (
                            <button
                                key={model.id}
                                data-model-id={model.id}
                                onClick={() => {
                                    onSelect(model.id);
                                    setIsOpen(false);
                                }}
                                className={`
                                    mx-1 flex items-center gap-1.5 rounded-md px-2 py-1.5 text-left
                                    transition-colors duration-150
                                    ${isSelected
                                        ? 'bg-[var(--accent-primary)]/10 text-[var(--fg-primary)]'
                                        : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)]'
                                    }
                                `}
                            >
                                <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                                    {getModelIcon(model.id)}
                                </div>
                                <div className="flex flex-col min-w-0 flex-1">
                                    <span className="truncate text-[10px] font-medium">
                                        {model.name}
                                    </span>
                                    {model.description && (
                                        <span className="text-[9px] text-[var(--fg-tertiary)] truncate opacity-80">
                                            {model.description}
                                        </span>
                                    )}
                                </div>
                                {isSelected && <Check className="w-2.5 h-2.5 text-[var(--accent-primary)] shrink-0" />}
                            </button>
                        );
                    })}
                    {openaiCompatModels.length > 0 && (
                        <div className="border-t border-[var(--border-subtle)] px-2 pt-2 text-[8px] font-semibold uppercase tracking-[0.16em] text-[var(--fg-tertiary)]">
                            Local Server
                        </div>
                    )}
                    {openaiCompatModels.map(model => {
                        const isSelected = model.id === selectedId;
                        return (
                            <button
                                key={model.id}
                                data-model-id={model.id}
                                onClick={() => {
                                    onSelect(model.id);
                                    setIsOpen(false);
                                }}
                                className={`
                                    mx-1 flex items-center gap-1.5 rounded-md px-2 py-1.5 text-left
                                    transition-colors duration-150
                                    ${isSelected
                                        ? 'bg-[var(--accent-primary)]/10 text-[var(--fg-primary)]'
                                        : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)]'
                                    }
                                `}
                            >
                                <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                                    {getModelIcon(model.id)}
                                </div>
                                <div className="flex flex-col min-w-0 flex-1">
                                    <span className="truncate text-[10px] font-medium">
                                        {model.name}
                                    </span>
                                    {model.description && (
                                        <span className="text-[9px] text-[var(--fg-tertiary)] truncate opacity-80">
                                            {model.description}
                                        </span>
                                    )}
                                </div>
                                {isSelected && <Check className="w-2.5 h-2.5 text-[var(--accent-primary)] shrink-0" />}
                            </button>
                        );
                    })}
                </div>
            )}
        </div>
    );
};

export const CompactModelSelector = React.memo(CompactModelSelectorComponent);
