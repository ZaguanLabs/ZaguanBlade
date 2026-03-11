import React, { useState, useRef, useEffect, useMemo } from 'react';
import { ModelInfo } from '../types/chat';
import { ChevronDown, Check, Box, Cpu, Sparkles, BrainCircuit } from 'lucide-react';
import { ThemedDropdownEmptyState, ThemedDropdownScrollArea, ThemedDropdownSectionLabel, ThemedDropdownSurface } from './ui/ThemedDropdown';

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

    const renderModelItem = (model: ModelInfo) => {
        const isSelected = model.id === selectedId;

        return (
            <button
                key={model.id}
                type="button"
                data-model-id={model.id}
                title={model.description ? `${model.name} — ${model.description}` : model.name}
                onClick={() => {
                    onSelect(model.id);
                    setIsOpen(false);
                }}
                className={`
                    group flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-left transition-colors duration-150 focus:outline-none
                    ${isSelected
                        ? 'bg-[color-mix(in_srgb,var(--accent-primary)_12%,var(--bg-surface))] text-(--fg-primary)'
                        : 'text-(--fg-secondary) hover:bg-(--bg-surface-hover) focus:bg-(--bg-surface-hover)'
                    }
                `}
            >
                <div className="flex min-w-0 flex-1 items-center gap-1.5">
                    <div className="flex h-4 w-4 shrink-0 items-center justify-center">
                        {getModelIcon(model.id)}
                    </div>
                    <div className={`truncate text-[10px] leading-none ${isSelected ? 'font-semibold text-(--fg-primary)' : 'font-medium'}`}>
                        {model.name}
                    </div>
                </div>
                <div className={`flex h-3.5 w-3.5 shrink-0 items-center justify-center ${isSelected ? 'text-(--accent-primary)' : 'text-transparent group-hover:text-(--fg-tertiary)'}`}>
                    <Check className="h-3 w-3" />
                </div>
            </button>
        );
    };

    return (
        <div className="relative w-full" ref={containerRef}>
            <button
                type="button"
                onClick={() => !disabled && setIsOpen(!isOpen)}
                disabled={disabled}
                className={`
                    w-full flex items-center justify-between gap-1.5 rounded-md border border-(--border-subtle) bg-(--bg-app) px-2 py-1
                    transition-colors duration-150
                    ${disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}
                `}
            >
                <div className="flex items-center gap-1 overflow-hidden">
                    <div className="flex h-4.5 w-4.5 shrink-0 items-center justify-center rounded-sm border border-(--border-subtle) bg-(--bg-surface)">
                        {selectedModel ? getModelIcon(selectedModel.id) : <Box className="w-3 h-3" />}
                    </div>
                    <span className="truncate text-[10px] font-medium text-(--fg-secondary)">
                        {selectedModel?.name || 'Select Model'}
                    </span>
                </div>
                <ChevronDown className={`h-2.5 w-2.5 text-(--fg-tertiary) transition-transform duration-200 ${isOpen ? 'rotate-180' : ''}`} />
            </button>

            {isOpen && (
                <ThemedDropdownSurface
                    onWheel={(event) => event.stopPropagation()}
                    className="fixed bottom-[52px] right-2 z-110 w-64 animate-in fade-in zoom-in-95 duration-100 origin-bottom-right"
                    style={{
                        overscrollBehavior: 'contain',
                    }}
                >
                    {models.length === 0 && (
                        <ThemedDropdownEmptyState className="px-2 py-1.5 text-[9px]">
                            No models available
                        </ThemedDropdownEmptyState>
                    )}
                    <ThemedDropdownScrollArea ref={dropdownRef} className="max-h-[240px] space-y-0.5 pr-0.5">
                        {cloudModels.map(renderModelItem)}
                        {ollamaModels.length > 0 && (
                            <ThemedDropdownSectionLabel className="px-2 pt-1.5 text-[8px]">
                                Ollama
                            </ThemedDropdownSectionLabel>
                        )}
                        {ollamaModels.map(renderModelItem)}
                        {openaiCompatModels.length > 0 && (
                            <ThemedDropdownSectionLabel className="px-2 pt-1.5 text-[8px]">
                                Local Server
                            </ThemedDropdownSectionLabel>
                        )}
                        {openaiCompatModels.map(renderModelItem)}
                    </ThemedDropdownScrollArea>
                </ThemedDropdownSurface>
            )}
        </div>
    );
};

export const CompactModelSelector = React.memo(CompactModelSelectorComponent);
