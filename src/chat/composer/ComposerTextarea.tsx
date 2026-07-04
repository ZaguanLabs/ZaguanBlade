import React, { forwardRef, useCallback, useEffect, useImperativeHandle, useRef } from 'react';
import type { ComposerSuggestion } from './MentionSuggestions';
import type { WorkspacePathMatch } from './usePathSuggestions';

// Native textarea undo is broken for controlled inputs under webkit2gtk (React
// rewrites `.value`, which clears the browser's undo stack), so the composer
// keeps its own history. Changes within the group window coalesce into one undo
// step so Ctrl+Z reverts a typing burst, not one character at a time.
const UNDO_GROUP_MS = 400;
const UNDO_MAX_ENTRIES = 100;

interface UndoEntry {
    value: string;
    start: number;
    end: number;
}

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
    const undoStackRef = useRef<UndoEntry[]>([]);
    const redoStackRef = useRef<UndoEntry[]>([]);
    const prevTextRef = useRef<string>(text);
    // Selection as of just BEFORE the current mutation (captured on keydown /
    // select), so an undo restores the cursor to where the change happened.
    const lastSelectionRef = useRef<{ start: number; end: number }>({ start: 0, end: 0 });
    const lastSnapshotAtRef = useRef<number>(0);
    const applyingHistoryRef = useRef(false);
    const pendingSelectionRef = useRef<{ start: number; end: number } | null>(null);

    // Observes ALL text changes — typing and programmatic (prefill, mention
    // insert, history navigation, clear-on-send) — so every path is undoable.
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
        const now = Date.now();
        if (now - lastSnapshotAtRef.current > UNDO_GROUP_MS || undoStackRef.current.length === 0) {
            const prev = prevTextRef.current;
            undoStackRef.current.push({
                value: prev,
                start: Math.min(lastSelectionRef.current.start, prev.length),
                end: Math.min(lastSelectionRef.current.end, prev.length),
            });
            if (undoStackRef.current.length > UNDO_MAX_ENTRIES) {
                undoStackRef.current.shift();
            }
        }
        lastSnapshotAtRef.current = now;
        redoStackRef.current = [];
        prevTextRef.current = text;
    }, [text]);

    const applyHistoryEntry = useCallback((entry: UndoEntry, pushOnto: UndoEntry[]) => {
        const textarea = textareaRef.current;
        if (entry.value === prevTextRef.current) {
            // Value identical (a coalesced burst netted out to no change):
            // setText would not re-render, so apply only the cursor and leave
            // the applying/pending flags untouched to avoid dangling state.
            if (textarea) {
                const max = textarea.value.length;
                textarea.setSelectionRange(Math.min(entry.start, max), Math.min(entry.end, max));
            }
            lastSnapshotAtRef.current = 0;
            return;
        }
        pushOnto.push({
            value: prevTextRef.current,
            start: textarea?.selectionStart ?? prevTextRef.current.length,
            end: textarea?.selectionEnd ?? prevTextRef.current.length,
        });
        applyingHistoryRef.current = true;
        pendingSelectionRef.current = { start: entry.start, end: entry.end };
        // A change right after undo/redo must start a fresh group, not coalesce.
        lastSnapshotAtRef.current = 0;
        setText(entry.value);
        onTriggerChange(null);
    }, [onTriggerChange, setText]);

    const undo = useCallback(() => {
        const entry = undoStackRef.current.pop();
        if (entry === undefined) {
            return;
        }
        applyHistoryEntry(entry, redoStackRef.current);
    }, [applyHistoryEntry]);

    const redo = useCallback(() => {
        const entry = redoStackRef.current.pop();
        if (entry === undefined) {
            return;
        }
        applyHistoryEntry(entry, undoStackRef.current);
    }, [applyHistoryEntry]);

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
                setText(nextText);
                const trigger = detectPathTrigger(nextText, event.currentTarget.selectionStart);
                onTriggerChange(trigger?.query ?? null);
            }}
            onPaste={handlePaste}
            onKeyDown={(event) => {
                // Keydown fires before the mutation — remember the pre-change
                // selection for the undo snapshot taken when `text` updates.
                lastSelectionRef.current = {
                    start: event.currentTarget.selectionStart,
                    end: event.currentTarget.selectionEnd,
                };
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
