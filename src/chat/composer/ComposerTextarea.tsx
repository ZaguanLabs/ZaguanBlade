import React, { forwardRef, useCallback, useEffect, useImperativeHandle, useRef } from 'react';
import type { ComposerSuggestion } from './MentionSuggestions';
import type { WorkspacePathMatch } from './usePathSuggestions';
import { ComposerUndoModel } from './composerUndo';

export interface ComposerTextareaHandle {
    focus: () => void;
    resize: () => void;
}

interface ComposerTextareaProps {
    text: string;
    setText: (value: string) => void;
    disabled?: boolean;
    placeholder: string;
    ariaLabel: string;
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
    ariaLabel,
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
    // Native textarea undo is broken for controlled inputs (React rewrites
    // `.value`, clearing the browser's undo stack — facebook/react #17494), so
    // the composer keeps its own history with Monaco-style word-at-most
    // grouping. All rules live in ComposerUndoModel (see composerUndo.ts).
    const undoModelRef = useRef<ComposerUndoModel | null>(null);
    if (undoModelRef.current === null) {
        undoModelRef.current = new ComposerUndoModel();
    }
    const undoModel = undoModelRef.current;
    const prevTextRef = useRef<string>(text);
    // Selection just BEFORE the current mutation (captured on keydown/paste,
    // which fire before the DOM change) — becomes the undo entry's cursor.
    const lastSelectionRef = useRef<{ start: number; end: number }>({ start: 0, end: 0 });
    // Delete direction of the pending keystroke (Backspace vs Delete both
    // leave the cursor at the change start, so the diff can't tell them apart).
    const deleteDirectionRef = useRef<'backward' | 'forward' | null>(null);
    const isComposingRef = useRef(false);
    const applyingHistoryRef = useRef(false);
    const pendingSelectionRef = useRef<{ start: number; end: number } | null>(null);

    // Handles cursor restoration after undo/redo, and records PROGRAMMATIC
    // text changes (prefill, mention insert, history navigation, clear-on-send
    // — anything that did not come through onChange) as isolated undo steps.
    useEffect(() => {
        const textarea = textareaRef.current;
        if (pendingSelectionRef.current && textarea) {
            const { start, end } = pendingSelectionRef.current;
            pendingSelectionRef.current = null;
            const max = textarea.value.length;
            textarea.setSelectionRange(Math.min(start, max), Math.min(end, max));
        }
        if (text === prevTextRef.current) {
            return;
        }
        if (applyingHistoryRef.current) {
            applyingHistoryRef.current = false;
            prevTextRef.current = text;
            return;
        }
        undoModel.recordProgrammaticChange(prevTextRef.current, text, lastSelectionRef.current);
        prevTextRef.current = text;
    }, [text, undoModel]);

    const applyHistoryEntry = useCallback((entry: { value: string; start: number; end: number }) => {
        if (entry.value === prevTextRef.current) {
            // Identical value: setText would not re-render, so apply only the
            // cursor and leave the applying/pending flags untouched.
            const textarea = textareaRef.current;
            if (textarea) {
                const max = textarea.value.length;
                textarea.setSelectionRange(Math.min(entry.start, max), Math.min(entry.end, max));
            }
            return;
        }
        applyingHistoryRef.current = true;
        pendingSelectionRef.current = { start: entry.start, end: entry.end };
        setText(entry.value);
        onTriggerChange(null);
    }, [onTriggerChange, setText]);

    const currentSnapshot = useCallback(() => {
        const textarea = textareaRef.current;
        const value = prevTextRef.current;
        return {
            value,
            start: Math.min(textarea?.selectionStart ?? value.length, value.length),
            end: Math.min(textarea?.selectionEnd ?? value.length, value.length),
        };
    }, []);

    const undo = useCallback(() => {
        const entry = undoModel.undo(currentSnapshot());
        if (entry) {
            applyHistoryEntry(entry);
        }
    }, [applyHistoryEntry, currentSnapshot, undoModel]);

    const redo = useCallback(() => {
        const entry = undoModel.redo(currentSnapshot());
        if (entry) {
            applyHistoryEntry(entry);
        }
    }, [applyHistoryEntry, currentSnapshot, undoModel]);

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
        // Paste fires before the mutation — capture the pre-change selection so
        // an undo of this paste restores the cursor to the paste point.
        lastSelectionRef.current = {
            start: event.currentTarget.selectionStart,
            end: event.currentTarget.selectionEnd,
        };
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
                undoModel.recordChange(
                    prevTextRef.current,
                    nextText,
                    lastSelectionRef.current,
                    deleteDirectionRef.current,
                    isComposingRef.current,
                );
                prevTextRef.current = nextText;
                setText(nextText);
                const trigger = detectPathTrigger(nextText, event.currentTarget.selectionStart);
                onTriggerChange(trigger?.query ?? null);
            }}
            onCompositionStart={() => {
                isComposingRef.current = true;
            }}
            onCompositionEnd={() => {
                isComposingRef.current = false;
                // The committed composition is one atomic undo step; following
                // input starts a fresh group.
                undoModel.closeGroup();
            }}
            onBlur={() => {
                // Focus loss ends the current undo group (Lexical terminator).
                undoModel.closeGroup();
            }}
            onPaste={handlePaste}
            onKeyDown={(event) => {
                // Keydown fires before the mutation — remember the pre-change
                // selection for the undo snapshot taken when `text` updates.
                lastSelectionRef.current = {
                    start: event.currentTarget.selectionStart,
                    end: event.currentTarget.selectionEnd,
                };
                deleteDirectionRef.current = event.key === 'Backspace'
                    ? 'backward'
                    : event.key === 'Delete'
                        ? 'forward'
                        : null;
                const combo = event.ctrlKey || event.metaKey;
                if (combo && !event.altKey && event.key.toLowerCase() === 'z') {
                    // Always claim the shortcut: webkit's native undo fights the
                    // controlled value and must never run, even on an empty stack.
                    event.preventDefault();
                    if (event.shiftKey) {
                        redo();
                    } else {
                        undo();
                    }
                    return;
                }
                if (combo && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 'y') {
                    event.preventDefault();
                    redo();
                    return;
                }
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
            aria-label={ariaLabel}
            disabled={disabled}
            rows={1}
            className="min-h-[88px] max-h-[360px] w-full resize-none overflow-y-auto bg-transparent px-3 pb-3 pt-2.5 pr-14 text-[13px] font-medium leading-[18px] text-(--fg-primary) outline-none placeholder-(--fg-tertiary)"
            style={{ fontSize: 'var(--chat-content-font-size, 13px)', lineHeight: 1.4 }}
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
