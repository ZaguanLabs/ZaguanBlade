import React, { useState, useRef, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Send, Square, Globe, Search, BookOpen } from 'lucide-react';
import { CompactModelSelector } from './CompactModelSelector';
import type { ModelInfo } from '../types/chat';

const COMMANDS = [
    { name: 'web', description: 'Fetch a URL', icon: Globe },
    { name: 'search', description: 'Search the web', icon: Search },
    { name: 'research', description: 'Deep research', icon: BookOpen },
];

interface CommandCenterProps {
    onSend: (text: string) => void;
    onStop?: () => void;
    disabled?: boolean;
    loading?: boolean;
    models: ModelInfo[];
    selectedModelId: string;
    setSelectedModelId: (modelId: string) => void;
}

export const CommandCenter: React.FC<CommandCenterProps> = ({ 
    onSend, 
    onStop, 
    disabled, 
    loading,
    models,
    selectedModelId,
    setSelectedModelId
}) => {
    const { t } = useTranslation();
    const [text, setText] = useState('');
    const [showCommands, setShowCommands] = useState(false);
    const [selectedCommandIndex, setSelectedCommandIndex] = useState(0);
    const [commandFilter, setCommandFilter] = useState('');
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const popupRef = useRef<HTMLDivElement>(null);

    // Filter commands based on what user typed after @
    const filteredCommands = COMMANDS.filter(cmd => 
        cmd.name.toLowerCase().startsWith(commandFilter.toLowerCase())
    );

    useEffect(() => {
        if (textareaRef.current) {
            textareaRef.current.style.height = 'auto';
            textareaRef.current.style.height = textareaRef.current.scrollHeight + 'px';
        }
    }, [text]);

    // Detect @ and show command popup
    const handleTextChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const newText = e.target.value;
        setText(newText);

        // Find if we're typing a command (@ at start or after space)
        const cursorPos = e.target.selectionStart;
        const textBeforeCursor = newText.slice(0, cursorPos);
        const lastAtIndex = textBeforeCursor.lastIndexOf('@');
        
        if (lastAtIndex !== -1) {
            // Check if @ is at start or after whitespace
            const charBefore = lastAtIndex > 0 ? textBeforeCursor[lastAtIndex - 1] : ' ';
            if (charBefore === ' ' || charBefore === '\n' || lastAtIndex === 0) {
                const afterAt = textBeforeCursor.slice(lastAtIndex + 1);
                // Only show if no space after the partial command
                if (!afterAt.includes(' ')) {
                    setCommandFilter(afterAt);
                    setShowCommands(true);
                    setSelectedCommandIndex(0);
                    return;
                }
            }
        }
        setShowCommands(false);
    }, []);

    // Insert selected command
    const insertCommand = useCallback((commandName: string) => {
        const cursorPos = textareaRef.current?.selectionStart || 0;
        const textBeforeCursor = text.slice(0, cursorPos);
        const textAfterCursor = text.slice(cursorPos);
        const lastAtIndex = textBeforeCursor.lastIndexOf('@');
        
        if (lastAtIndex !== -1) {
            const newText = textBeforeCursor.slice(0, lastAtIndex) + `@${commandName} ` + textAfterCursor;
            setText(newText);
            setShowCommands(false);
            
            // Focus and set cursor position after command
            setTimeout(() => {
                if (textareaRef.current) {
                    const newCursorPos = lastAtIndex + commandName.length + 2;
                    textareaRef.current.focus();
                    textareaRef.current.setSelectionRange(newCursorPos, newCursorPos);
                }
            }, 0);
        }
    }, [text]);

    const handleKeyDown = (e: React.KeyboardEvent) => {
        // Handle command popup navigation
        if (showCommands && filteredCommands.length > 0) {
            if (e.key === 'ArrowDown') {
                e.preventDefault();
                setSelectedCommandIndex(prev => 
                    prev < filteredCommands.length - 1 ? prev + 1 : 0
                );
                return;
            }
            if (e.key === 'ArrowUp') {
                e.preventDefault();
                setSelectedCommandIndex(prev => 
                    prev > 0 ? prev - 1 : filteredCommands.length - 1
                );
                return;
            }
            if (e.key === 'Enter' || e.key === 'Tab') {
                e.preventDefault();
                insertCommand(filteredCommands[selectedCommandIndex].name);
                return;
            }
            if (e.key === 'Escape') {
                e.preventDefault();
                setShowCommands(false);
                return;
            }
        }

        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            if (text.trim() && !disabled && !loading) {
                onSend(text);
                setText('');
            }
        }
    };

    return (
        <div className="shrink-0 border-t border-[var(--border-subtle)] bg-[var(--bg-app)]">
            <div className="p-3 pb-5">
                <div className="bg-[var(--bg-surface)] rounded-md border border-[var(--border-subtle)]">
                    {/* Model Selector */}
                    <div className="border-b border-[var(--border-subtle)]/50 px-1 py-0.5">
                        <CompactModelSelector
                            models={models}
                            selectedId={selectedModelId || ''}
                            onSelect={setSelectedModelId}
                            disabled={disabled}
                        />
                    </div>

                    {/* Chat Input */}
                    <div className={`relative transition-colors ${loading ? 'bg-[var(--bg-surface)]' : ''}`}>
                        {/* Command Autocomplete Popup */}
                        {showCommands && filteredCommands.length > 0 && (
                            <div 
                                ref={popupRef}
                                className="absolute bottom-full left-0 right-0 mb-1 mx-2 bg-[var(--bg-surface)] border border-[var(--border-subtle)] rounded-md shadow-lg overflow-hidden z-50"
                            >
                                {filteredCommands.map((cmd, idx) => {
                                    const Icon = cmd.icon;
                                    return (
                                        <button
                                            key={cmd.name}
                                            onClick={() => insertCommand(cmd.name)}
                                            className={`w-full flex items-center gap-3 px-3 py-2 text-left transition-colors ${
                                                idx === selectedCommandIndex 
                                                    ? 'bg-[var(--accent-primary)]/15 text-[var(--fg-primary)]' 
                                                    : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)]'
                                            }`}
                                        >
                                            <Icon className="w-4 h-4 text-[var(--accent-primary)]" />
                                            <div className="flex-1">
                                                <span className="text-xs font-semibold">@{cmd.name}</span>
                                                <span className="text-xs text-[var(--fg-tertiary)] ml-2">{cmd.description}</span>
                                            </div>
                                        </button>
                                    );
                                })}
                            </div>
                        )}
                        <textarea
                            ref={textareaRef}
                            value={text}
                            onChange={handleTextChange}
                            onKeyDown={handleKeyDown}
                            placeholder={t('chat.inputPlaceholder')}
                            className="w-full bg-transparent text-[var(--fg-primary)] p-3 pr-10 outline-none resize-none min-h-[42px] max-h-[200px] text-xs font-mono placeholder-[var(--fg-tertiary)]"
                            rows={1}
                            disabled={disabled}
                        />
                        <button
                            onClick={() => {
                                const showStop = loading && !text.trim();
                                if (showStop && onStop) {
                                    onStop();
                                } else if (text.trim() && !disabled) {
                                    onSend(text);
                                    setText('');
                                }
                            }}
                            disabled={(!text.trim() && !loading) || disabled}
                            className={`absolute right-2 bottom-2 p-1.5 transition-colors rounded hover:bg-[var(--bg-surface-hover)] ${loading && !text.trim()
                                ? 'text-red-400'
                                : 'text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)] disabled:opacity-30 disabled:cursor-not-allowed'
                                }`}
                        >
                            {loading && !text.trim() ? (
                                <Square className="w-3.5 h-3.5 fill-current animate-pulse" />
                            ) : (
                                <Send className="w-3.5 h-3.5" />
                            )}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
};
