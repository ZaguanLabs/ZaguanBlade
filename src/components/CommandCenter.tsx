import React, { useState, useRef, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Send, Square, BookOpen } from 'lucide-react';
import { CompactModelSelector } from './CompactModelSelector';
import type { ModelInfo } from '../types/chat';

const COMMANDS = [
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

// Optimized rendering component for the formatted text overlay
const FormattedOverlay: React.FC<{ text: string }> = React.memo(({ text }) => {
    if (!text.includes('@')) return null;

    const parts: React.ReactNode[] = [];
    const regex = /@(\w+)/g;
    let lastIndex = 0;
    let match;

    while ((match = regex.exec(text)) !== null) {
        if (match.index > lastIndex) {
            parts.push(
                <span key={`text-${lastIndex}`} className="font-sans">
                    {text.slice(lastIndex, match.index)}
                </span>
            );
        }
        parts.push(
            <span
                key={`cmd-${match.index}`}
                className="text-[var(--accent-primary)] font-semibold"
            >
                {match[0]}
            </span>
        );
        lastIndex = regex.lastIndex;
    }

    if (lastIndex < text.length) {
        parts.push(
            <span key={`text-${lastIndex}`} className="font-sans">
                {text.slice(lastIndex)}
            </span>
        );
    }

    return <>{parts}</>;
});
FormattedOverlay.displayName = 'FormattedOverlay';

const CommandCenterComponent: React.FC<CommandCenterProps> = ({
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


    // Handle scroll syncing between textarea and overlay
    const handleScroll = useCallback((e: React.UIEvent<HTMLTextAreaElement>) => {
        if (popupRef.current) {
            popupRef.current.scrollTop = e.currentTarget.scrollTop;
        }
    }, []);

    const hasCommand = text.includes('@');

    // Filter commands based on what user typed after @
    const filteredCommands = React.useMemo(() =>
        COMMANDS.filter(cmd =>
            cmd.name.toLowerCase().startsWith(commandFilter.toLowerCase())
        ),
        [commandFilter]);

    // Detect @ and show command popup
    const handleTextChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const newText = e.target.value;
        const textarea = e.target;
        
        // Adjust height - do this BEFORE setState to avoid double layout
        textarea.style.height = '42px';
        textarea.style.height = `${Math.min(textarea.scrollHeight, 400)}px`;
        
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
                        {/* Formatted text overlay - only rendered when commands are present */}
                        {hasCommand && (
                            <div
                                ref={popupRef}
                                className="absolute inset-0 p-3 pr-10 pointer-events-none whitespace-pre-wrap break-words text-xs font-sans leading-relaxed overflow-hidden"
                                style={{ color: 'var(--fg-secondary)' }}
                            >
                                <FormattedOverlay text={text} />
                            </div>
                        )}
                        {/* Command Autocomplete Popup */}
                        {showCommands && (
                            <div
                                className="absolute bottom-full left-0 right-0 mb-1 mx-2 bg-[var(--bg-surface)] border border-[var(--border-subtle)] rounded-md shadow-lg overflow-hidden z-50"
                            >
                                {filteredCommands.map((cmd, idx) => {
                                    const Icon = cmd.icon;
                                    return (
                                        <button
                                            key={cmd.name}
                                            onClick={() => insertCommand(cmd.name)}
                                            className={`w-full flex items-center gap-3 px-3 py-2 text-left transition-colors ${idx === selectedCommandIndex
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
                            onScroll={handleScroll}
                            placeholder={t('chat.inputPlaceholder')}
                            className={`w-full bg-transparent p-3 pr-10 outline-none resize-none min-h-[42px] max-h-[400px] overflow-y-auto text-xs font-sans placeholder-[var(--fg-tertiary)] leading-relaxed relative z-10 ${hasCommand ? 'text-transparent caret-[var(--fg-secondary)]' : 'text-[var(--fg-secondary)]'
                                }`}
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
                            className={`absolute right-2 bottom-2 p-1.5 transition-colors rounded hover:bg-[var(--bg-surface-hover)] z-20 ${loading && !text.trim()
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

export const CommandCenter = React.memo(CommandCenterComponent);
