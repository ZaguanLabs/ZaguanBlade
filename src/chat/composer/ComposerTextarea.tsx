import React, { forwardRef, useCallback, useEffect, useImperativeHandle, useRef } from 'react';
import type { ComposerSuggestion } from './MentionSuggestions';
import type { WorkspacePathMatch } from './usePathSuggestions';

export interface ComposerTextareaHandle {
    focus: () => void;
    resize: () => void;
}

interface ComposerTextareaProps {
    text: string;
    setText: (value: string) => void;
    disabled?: boolean;
    placeholder: string;
    showSuggestions: boolean;
    suggestions: ComposerSuggestion[];
    selectedSuggestionIndex: number;
    onTriggerChange: (query: string | null) => void;
    onSelectSuggestion: (suggestion: ComposerSuggestion) => void;
    onSubmit: () => void;
    onPasteImages: (files: File[]) => void;
}

function detectPathTrigger(text: string, cursor: number): { query: string; start: number; end: number } | null {
    const prefix = text.slice(0, cursor);
    const match = /@([^\s@]*)$/.exec(prefix);
    if (!match) {
        return null;
    }
    return {
        query: match[1],
        start: cursor - match[0].length,
        end: cursor,
    };
}

export const ComposerTextarea = forwardRef<ComposerTextareaHandle, ComposerTextareaProps>(({
    text,
    setText,
    disabled,
    placeholder,
    showSuggestions,
    suggestions,
    selectedSuggestionIndex,
    onTriggerChange,
    onSelectSuggestion,
    onSubmit,
    onPasteImages,
}, ref) => {
    const textareaRef = useRef<HTMLTextAreaElement>(null);

    const resize = useCallback(() => {
        const textarea = textareaRef.current;
        if (!textarea) {
            return;
        }
        textarea.style.height = 'auto';
        textarea.style.height = `${Math.min(textarea.scrollHeight, 360)}px`;
    }, []);

    useImperativeHandle(ref, () => ({
        focus: () => textareaRef.current?.focus(),
        resize,
    }), [resize]);

    useEffect(() => {
        resize();
    }, [resize, text]);

    return (
        <textarea
            ref={textareaRef}
            value={text}
            onChange={(event) => {
                const nextText = event.currentTarget.value;
                setText(nextText);
                const trigger = detectPathTrigger(nextText, event.currentTarget.selectionStart);
                onTriggerChange(trigger?.query ?? null);
            }}
            onPaste={(event) => {
                const files = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith('image/') || /\.(png|jpe?g|webp|gif)$/i.test(file.name));
                if (files.length > 0) {
                    event.preventDefault();
                    onPasteImages(files);
                }
            }}
            onKeyDown={(event) => {
                if (showSuggestions && suggestions.length > 0) {
                    if (event.key === 'Enter' || event.key === 'Tab') {
                        event.preventDefault();
                        onSelectSuggestion(suggestions[selectedSuggestionIndex]);
                        return;
                    }
                    if (event.key === 'Escape') {
                        event.preventDefault();
                        onTriggerChange(null);
                        return;
                    }
                }
                if (event.key === 'Enter' && !event.shiftKey) {
                    event.preventDefault();
                    onSubmit();
                }
            }}
            placeholder={placeholder}
            disabled={disabled}
            rows={1}
            className="min-h-[88px] max-h-[360px] w-full resize-none overflow-y-auto bg-transparent px-3 pb-3 pt-2.5 pr-14 text-[13px] font-medium leading-[18px] text-(--fg-primary) outline-none placeholder-(--fg-tertiary)"
        />
    );
});

ComposerTextarea.displayName = 'ComposerTextarea';

export function replaceActiveTrigger(text: string, cursor: number, suggestion: WorkspacePathMatch): { text: string; cursor: number } {
    const trigger = detectPathTrigger(text, cursor);
    if (!trigger) {
        return { text, cursor };
    }
    const replacement = `@${suggestion.path}${suggestion.is_dir ? '/' : ' '}`;
    return {
        text: `${text.slice(0, trigger.start)}${replacement}${text.slice(trigger.end)}`,
        cursor: trigger.start + replacement.length,
    };
}
