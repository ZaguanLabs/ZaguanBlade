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
    onNavigateHistory: (direction: 'previous' | 'next') => boolean;
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

function looksLikeImageFile(file: File): boolean {
    return file.type.startsWith('image/') || /\.(png|jpe?g|webp|gif)$/i.test(file.name) || file.type === '';
}

function dedupeFileKey(file: File): string {
    return `${file.name}:${file.size}:${file.lastModified}:${file.type}`;
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
    onNavigateHistory,
    onPasteImages,
}, ref) => {
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const lastScrollHeightRef = useRef<number>(0);

    const resize = useCallback(() => {
        const textarea = textareaRef.current;
        if (!textarea) {
            return;
        }
        textarea.style.height = 'auto';
        textarea.style.height = `${Math.min(textarea.scrollHeight, 360)}px`;
        lastScrollHeightRef.current = textarea.scrollHeight;
    }, []);

    useImperativeHandle(ref, () => ({
        focus: () => textareaRef.current?.focus(),
        resize,
    }), [resize]);

    useEffect(() => {
        const textarea = textareaRef.current;
        if (!textarea) {
            return;
        }
        // Only resize if scrollHeight changed (content grew/shrunk)
        if (textarea.scrollHeight !== lastScrollHeightRef.current) {
            resize();
        }
    }, [resize, text]);

    const handlePaste = useCallback(async (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
        const filesFromItems = Array.from(event.clipboardData.items)
            .filter((item) => item.kind === 'file')
            .map((item) => item.getAsFile())
            .filter((file): file is File => !!file);
        const filesFromClipboard = Array.from(event.clipboardData.files);
        const fileMap = new Map<string, File>();

        for (const file of [...filesFromItems, ...filesFromClipboard]) {
            if (looksLikeImageFile(file)) {
                fileMap.set(dedupeFileKey(file), file);
            }
        }

        if (fileMap.size > 0) {
            event.preventDefault();
            onPasteImages(Array.from(fileMap.values()));
            return;
        }

        if (!navigator.clipboard?.read) {
            return;
        }

        try {
            const clipboardItems = await navigator.clipboard.read();
            const clipboardFiles: File[] = [];

            for (const item of clipboardItems) {
                const imageTypes = item.types.filter((type) => type.startsWith('image/'));
                for (const imageType of imageTypes) {
                    const blob = await item.getType(imageType);
                    const ext = imageType.split('/')[1] || 'png';
                    clipboardFiles.push(new File([blob], `clipboard-image.${ext}`, {
                        type: imageType,
                        lastModified: Date.now(),
                    }));
                }
            }

            if (clipboardFiles.length > 0) {
                event.preventDefault();
                onPasteImages(clipboardFiles);
            }
        } catch {
            return;
        }
    }, [onPasteImages]);

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
            onPaste={handlePaste}
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
                    return;
                }
                if (!showSuggestions && event.key === 'ArrowUp') {
                    const textarea = event.currentTarget;
                    if (textarea.selectionStart === textarea.selectionEnd && textarea.selectionStart === 0) {
                        if (onNavigateHistory('previous')) {
                            event.preventDefault();
                        }
                    }
                    return;
                }
                if (!showSuggestions && event.key === 'ArrowDown') {
                    const textarea = event.currentTarget;
                    if (textarea.selectionStart === textarea.selectionEnd && textarea.selectionStart === textarea.value.length) {
                        if (onNavigateHistory('next')) {
                            event.preventDefault();
                        }
                    }
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
