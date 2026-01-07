'use client';
import React, { useState, useRef, useEffect } from 'react';
import { useTranslations } from 'next-intl';
import { Send, Square } from 'lucide-react';

interface ChatInputProps {
    onSend: (text: string) => void;
    onStop?: () => void;
    disabled?: boolean;
    loading?: boolean;
}

export const ChatInput: React.FC<ChatInputProps> = ({ onSend, onStop, disabled, loading }) => {
    const t = useTranslations('chat');
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
        <div className="p-3 border-t border-zinc-800 bg-zinc-950 pb-5">
            <div className="max-w-4xl mx-auto relative bg-zinc-900/50 rounded-sm overflow-hidden border border-zinc-800 focus-within:border-emerald-500/30 ring-0 transition-colors">
                <textarea
                    ref={textareaRef}
                    value={text}
                    onChange={(e) => setText(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder={t('inputPlaceholder')}
                    className="w-full bg-transparent text-zinc-200 p-3 pr-10 outline-none resize-none min-h-[42px] max-h-[200px] text-sm font-mono placeholder-zinc-600"
                    rows={1}
                    disabled={disabled || loading}
                />
                <button
                    onClick={() => {
                        if (loading && onStop) {
                            onStop();
                        } else if (text.trim() && !disabled) {
                            onSend(text);
                            setText('');
                        }
                    }}
                    disabled={(!text.trim() && !loading) || disabled}
                    className={`absolute right-2 bottom-2 p-1.5 transition-colors rounded-sm active:translate-y-px ${loading
                        ? 'text-red-500 hover:text-red-400 hover:bg-red-500/10'
                        : 'text-zinc-500 hover:text-emerald-400 hover:bg-zinc-800 disabled:opacity-30 disabled:cursor-not-allowed'
                        }`}
                >
                    {loading ? (
                        <Square className="w-4 h-4 fill-current animate-pulse" />
                    ) : (
                        <Send className="w-4 h-4" />
                    )}
                </button>
            </div>
        </div>
    );
};
