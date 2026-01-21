import React, { useState, useRef, useEffect } from 'react';
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
                    w-full flex items-center justify-between px-2 py-1
                    bg-transparent hover:bg-[var(--bg-surface-hover)]/30
                    border-0 rounded transition-colors duration-150
                    ${disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}
                `}
            >
                <div className="flex items-center gap-1.5 overflow-hidden">
                    <div className="shrink-0">
                        {selectedModel ? getModelIcon(selectedModel.id) : <Box className="w-3 h-3" />}
                    </div>
                    <span className="text-[10px] font-medium text-[var(--fg-secondary)] truncate">
                        {selectedModel?.name || 'Select Model'}
                    </span>
                </div>
                <ChevronDown className={`w-3 h-3 text-[var(--fg-tertiary)] transition-transform duration-200 ${isOpen ? 'rotate-180' : ''}`} />
            </button>

            {isOpen && (
                <div className="fixed bottom-[60px] right-3 w-80 py-1 bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded-lg shadow-xl z-[100] max-h-[300px] overflow-y-auto flex flex-col gap-0.5 animate-in fade-in zoom-in-95 duration-100 origin-bottom-right" style={{ boxShadow: '0 8px 30px rgba(0, 0, 0, 0.4), 0 0 1px rgba(255, 255, 255, 0.1)' }}>
                    {models.length === 0 && (
                        <div className="px-2 py-1.5 text-[10px] text-[var(--fg-tertiary)] text-center italic">
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
                                    flex items-center gap-1.5 px-2 py-0.5 mx-1 rounded-sm text-left
                                    transition-colors duration-150
                                    ${isSelected
                                        ? 'bg-[var(--accent-primary)]/10 text-[var(--fg-primary)]'
                                        : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)]'
                                    }
                                `}
                            >
                                <div className="shrink-0">
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
