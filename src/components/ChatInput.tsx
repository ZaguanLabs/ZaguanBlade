import React, { useState, useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Send, Square } from 'lucide-react';

interface ChatInputProps {
    onSend: (text: string) => void;
    onStop?: () => void;
    disabled?: boolean;
    loading?: boolean;
}

export const ChatInput: React.FC<ChatInputProps> = ({ onSend, onStop, disabled, loading }) => {
    const { t } = useTranslation();
    const [text, setText] = useState('');
    const textareaRef = useRef<HTMLTextAreaElement>(null);

    useEffect(() => {
        if (textareaRef.current) {
            textareaRef.current.style.height = 'auto';
            textareaRef.current.style.height = textareaRef.current.scrollHeight + 'px';
        }
    }, [text]);

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            if (text.trim() && !disabled && !loading) {
                onSend(text);
                setText('');
            }
        }
    };

    return (
        <div className="p-3 border-t border-[var(--border-subtle)] bg-[var(--bg-app)] pb-5">
            <div className={`relative bg-[var(--bg-surface)] rounded-md overflow-hidden border transition-colors ${loading ? 'border-[var(--border-focus)]' : 'border-[var(--border-subtle)] focus-within:border-[var(--fg-secondary)]'}`}>
                <textarea
                    ref={textareaRef}
                    value={text}
                    onChange={(e) => setText(e.target.value)}
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
    );
};
