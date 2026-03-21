'use client';
import React, { useState, useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { ModelInfo } from '../types/chat';
import { ChevronDown, Check, Box, Cpu, Sparkles, BrainCircuit } from 'lucide-react';
import { ThemedDropdownEmptyState, ThemedDropdownScrollArea, ThemedDropdownSurface, themedDropdownItemClassName } from './ui/ThemedDropdown';

interface ModelSelectorProps {
    models: ModelInfo[];
    selectedId: string;
    onSelect: (id: string) => void;
    disabled?: boolean;
}

export const ModelSelector: React.FC<ModelSelectorProps> = ({ models, selectedId, onSelect, disabled }) => {
    const { t } = useTranslation();
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

    const renderModelItem = (model: ModelInfo) => {
        const isSelected = model.id === selectedId;

        return (
            <button
                key={model.id}
                type="button"
                onClick={() => {
                    onSelect(model.id);
                    setIsOpen(false);
                }}
                className={themedDropdownItemClassName(isSelected, 'px-3 py-3')}
            >
                <div className="flex items-start gap-3">
                    <div className={`mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-[calc(var(--panel-radius)-6px)] border ${
                        isSelected
                            ? 'border-(--accent-primary) bg-[color-mix(in_srgb,var(--accent-primary)_14%,var(--bg-app))]'
                            : 'border-(--border-subtle) bg-(--bg-app)'
                    }`}>
                        {getModelIcon(model.id)}
                    </div>
                    <div className="min-w-0 flex-1 space-y-1">
                        <div className="truncate text-xs font-semibold text-(--fg-primary)">
                            {model.name}
                        </div>
                        {model.description && (
                            <div className="truncate text-[10px] leading-relaxed text-(--fg-secondary)">
                                {model.description}
                            </div>
                        )}
                    </div>
                    <div className={`mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full border ${
                        isSelected
                            ? 'border-(--accent-primary) bg-[color-mix(in_srgb,var(--accent-primary)_16%,transparent)] text-(--accent-primary)'
                            : 'border-(--border-default) bg-(--bg-app) text-(--fg-tertiary)'
                    }`}>
                        <Check className={`h-3.5 w-3.5 transition-opacity duration-150 ${isSelected ? 'opacity-100' : 'opacity-0 group-hover:opacity-45'}`} />
                    </div>
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
                    w-full flex items-center justify-between px-3 py-1.5 
                    bg-(--bg-surface) hover:bg-(--bg-surface-hover)
                    border border-(--border-subtle) hover:border-(--border-default)
                    rounded transition-all duration-200 group
                    ${isOpen ? 'ring-1 ring-(--accent-primary)/50 border-(--accent-primary)/50' : ''}
                    ${disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}
                `}
            >
                <div className="flex items-center gap-2 overflow-hidden">
                    <div className="shrink-0 p-0.5 rounded bg-(--bg-app)/50 border border-(--border-subtle)">
                        {selectedModel ? getModelIcon(selectedModel.id) : <Box className="w-3 h-3" />}
                    </div>
                    <div className="flex flex-col items-start min-w-0">
                        <span className="text-[11px] font-medium text-(--fg-secondary) truncate w-full text-left">
                            {selectedModel?.name || t('chat.modelSelection')}
                        </span>
                    </div>
                </div>
                <ChevronDown className={`w-3 h-3 text-(--fg-tertiary) transition-transform duration-200 ${isOpen ? 'rotate-180' : ''}`} />
            </button>

            {isOpen && (
                <ThemedDropdownSurface
                    onWheel={(event) => event.stopPropagation()}
                    className="absolute top-full left-0 right-0 mt-1.5 animate-in fade-in zoom-in-95 duration-100 origin-top"
                    style={{
                        overscrollBehavior: 'contain',
                    }}
                >
                    {models.length === 0 && (
                        <ThemedDropdownEmptyState className="text-xs">
                            {t('chat.modelPicker.empty')}
                        </ThemedDropdownEmptyState>
                    )}
                    <ThemedDropdownScrollArea className="max-h-[300px]">
                        {models.map(renderModelItem)}
                    </ThemedDropdownScrollArea>
                </ThemedDropdownSurface>
            )}
        </div>
    );
};
