import React, { useState, useRef, useCallback, useMemo, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { readFile } from '@tauri-apps/plugin-fs';
import { Send, Square, BookOpen, Globe, FileText, Folder, Code2, Map as MapIcon } from 'lucide-react';
import { CompactModelSelector } from './CompactModelSelector';
import { FeatureMenu } from './FeatureMenu';
import { ImageAttachmentBar } from './ImageAttachmentBar';
import { WindowPicker } from './WindowPicker';
import { RegionSelector } from './RegionSelector';
import type { ChatMode, ComposerMention, ImageAttachment, ModelInfo, QueuedRequest } from '../types/chat';
import type { CaptureResult, WindowInfo } from '../types/screenshot';
import {
    createThumbnailDataUrl,
    extractBase64FromDataUrl,
    extractMimeTypeFromDataUrl,
    fileToDataUrl,
    getBase64ByteLength,
    validateImageByteLength,
    validateImageMimeType,
} from '../utils/imageUtils';
import { detectComposerTrigger, replaceTextRange } from '../utils/composerTriggers';

const COMMANDS = [
    { 
        name: 'web', 
        icon: Globe 
    },
    { 
        name: 'research', 
        icon: BookOpen 
    },
];

const CHAT_MODE_OPTIONS: Array<{
    value: ChatMode;
    icon: typeof Code2;
}> = [
    {
        value: 'code',
        icon: Code2,
    },
    {
        value: 'planning',
        icon: MapIcon,
    },
];

interface CommandCenterProps {
    onSend: (text: string, attachments?: ImageAttachment[], mentions?: ComposerMention[], mode?: ChatMode) => void;
    onStop?: () => void;
    disabled?: boolean;
    loading?: boolean;
    models: ModelInfo[];
    selectedModelId: string;
    setSelectedModelId: (modelId: string) => void;
    chatMode: ChatMode;
    setChatMode: (mode: ChatMode) => void;
    prefillRequest?: QueuedRequest | null;
    onPrefillConsumed?: () => void;
}

interface WorkspacePathMatch {
    path: string;
    is_dir: boolean;
}

function collectActivePathMentions(text: string, entries: WorkspacePathMatch[]): ComposerMention[] {
    return entries
        .filter((entry) => text.includes(`@${entry.path}`))
        .map((entry) => ({
            kind: 'path' as const,
            path: entry.path,
            is_dir: entry.is_dir,
        }));
}

type ComposerSuggestion =
    | { kind: 'command'; key: string; name: string; description: string; tooltip: string; icon: typeof Globe }
    | { kind: 'path'; key: string; path: string; isDir: boolean };

const CommandCenterComponent: React.FC<CommandCenterProps> = ({
    onSend,
    onStop,
    disabled,
    loading,
    models,
    selectedModelId,
    setSelectedModelId,
    chatMode,
    setChatMode,
    prefillRequest,
    onPrefillConsumed,
}) => {
    const { t } = useTranslation();
    const commandOptions = useMemo(() => COMMANDS.map((command) => ({
        ...command,
        description: command.name === 'web'
            ? t('chat.commands.webDescription')
            : t('chat.commands.researchDescription'),
        tooltip: command.name === 'web'
            ? t('chat.commands.webTooltip')
            : t('chat.commands.researchTooltip'),
    })), [t]);
    const chatModeOptions = useMemo(() => CHAT_MODE_OPTIONS.map((option) => ({
        ...option,
        label: option.value === 'code'
            ? t('chat.mode.codeLabel')
            : t('chat.mode.planningLabel'),
        description: option.value === 'code'
            ? t('chat.mode.codeDescription')
            : t('chat.mode.planningDescription'),
    })), [t]);
    const [text, setText] = useState('');
    const [showSuggestions, setShowSuggestions] = useState(false);
    const [selectedSuggestionIndex, setSelectedSuggestionIndex] = useState(0);
    const [mentionQuery, setMentionQuery] = useState('');
    const [pathSuggestions, setPathSuggestions] = useState<WorkspacePathMatch[]>([]);
    const [selectedPathMentions, setSelectedPathMentions] = useState<Record<string, WorkspacePathMatch>>({});
    const [attachments, setAttachments] = useState<ImageAttachment[]>([]);
    const [attachmentError, setAttachmentError] = useState<string | null>(null);
    const [windowPickerOpen, setWindowPickerOpen] = useState(false);
    const [windowPickerLoading, setWindowPickerLoading] = useState(false);
    const [windowPickerMode, setWindowPickerMode] = useState<'capture' | 'region'>('capture');
    const [capturableWindows, setCapturableWindows] = useState<WindowInfo[]>([]);
    const [regionSourceWindowId, setRegionSourceWindowId] = useState<number | null>(null);
    const [regionCapture, setRegionCapture] = useState<{
        dataUrl: string;
        width: number;
        height: number;
    } | null>(null);
    const [isCapturing, setIsCapturing] = useState(false);
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const resizeRafRef = useRef<number | null>(null);
    const resizeTextarea = useCallback((textarea?: HTMLTextAreaElement | null) => {
        if (!textarea) return;
        textarea.style.height = 'auto';
        textarea.style.height = `${Math.min(textarea.scrollHeight, 400)}px`;
    }, []);
    const scheduleTextareaResize = useCallback((textarea?: HTMLTextAreaElement | null) => {
        if (!textarea) return;
        if (resizeRafRef.current !== null) {
            cancelAnimationFrame(resizeRafRef.current);
        }
        resizeRafRef.current = requestAnimationFrame(() => {
            resizeRafRef.current = null;
            resizeTextarea(textarea);
        });
    }, [resizeTextarea]);
    const focusComposer = useCallback(() => {
        if (disabled || !textareaRef.current) return;
        const textarea = textareaRef.current;
        const end = textarea.value.length;
        textarea.focus();
        textarea.setSelectionRange(end, end);
        resizeTextarea(textarea);
    }, [disabled, resizeTextarea]);
    const isLocalOnly = useMemo(() => (
        models.length > 0
        && models.every((model) => model.provider === 'ollama' || model.provider === 'openai-compat')
    ), [models]);

    const isGlmModel = useMemo(() => {
        const id = selectedModelId.toLowerCase();
        return id.includes('glm-4') || id.includes('glm-5') || id.includes('glm4') || id.includes('glm5');
    }, [selectedModelId]);
    const activePathMentions = useMemo(
        () => collectActivePathMentions(text, Object.values(selectedPathMentions)),
        [selectedPathMentions, text]
    );
    const filteredCommands = useMemo(() =>
        commandOptions.filter(cmd =>
            cmd.name.toLowerCase().startsWith(mentionQuery.toLowerCase())
        ),
    [commandOptions, mentionQuery]);
    const suggestions = useMemo<ComposerSuggestion[]>(() => {
        const commandSuggestions: ComposerSuggestion[] = filteredCommands.map((cmd) => ({
            kind: 'command',
            key: `command:${cmd.name}`,
            name: cmd.name,
            description: cmd.description,
            tooltip: cmd.tooltip,
            icon: cmd.icon,
        }));
        const fileSuggestions: ComposerSuggestion[] = pathSuggestions.map((entry) => ({
            kind: 'path',
            key: `path:${entry.path}`,
            path: entry.path,
            isDir: entry.is_dir,
        }));

        return [...commandSuggestions, ...fileSuggestions];
    }, [filteredCommands, pathSuggestions]);

    // Cleanup pending resize RAF on unmount.
    useEffect(() => {
        return () => {
            if (resizeRafRef.current !== null) {
                cancelAnimationFrame(resizeRafRef.current);
            }
        };
    }, []);

    useEffect(() => {
        if (!prefillRequest) return;
        setText(prefillRequest.text);
        setAttachments(prefillRequest.attachments || []);
        setChatMode(prefillRequest.mode);
        setSelectedPathMentions(
            Object.fromEntries(
                (prefillRequest.mentions || []).map((mention) => [
                    mention.path,
                    { path: mention.path, is_dir: mention.is_dir },
                ])
            )
        );
        setAttachmentError(null);

        requestAnimationFrame(() => {
            focusComposer();
        });

        onPrefillConsumed?.();
    }, [focusComposer, prefillRequest, onPrefillConsumed, setChatMode]);

    useEffect(() => {
        if (!showSuggestions) {
            setPathSuggestions([]);
            return;
        }

        let cancelled = false;
        const timeoutId = window.setTimeout(async () => {
            try {
                const results = await invoke<WorkspacePathMatch[]>('search_workspace_paths', {
                    query: mentionQuery,
                    limit: 12,
                });
                if (!cancelled) {
                    setPathSuggestions(results);
                }
            } catch (error) {
                if (!cancelled) {
                    console.error('[CommandCenter] Failed to search workspace paths:', error);
                    setPathSuggestions([]);
                }
            }
        }, 120);

        return () => {
            cancelled = true;
            window.clearTimeout(timeoutId);
        };
    }, [mentionQuery, showSuggestions]);

    useEffect(() => {
        if (selectedSuggestionIndex >= suggestions.length) {
            setSelectedSuggestionIndex(0);
        }
    }, [selectedSuggestionIndex, suggestions.length]);

    useEffect(() => {
        const handleWindowKeyDown = (event: KeyboardEvent) => {
            if (!event.ctrlKey || event.shiftKey || event.altKey || event.metaKey) return;
            if (event.key.toLowerCase() !== 'l') return;
            event.preventDefault();
            focusComposer();
        };

        window.addEventListener('keydown', handleWindowKeyDown);
        return () => window.removeEventListener('keydown', handleWindowKeyDown);
    }, [focusComposer]);

    const appendImageFiles = useCallback(async (inputFiles: File[]) => {
        if (inputFiles.length === 0) return;

        if (isLocalOnly) {
            setAttachmentError(t('chat.imageNoSubscription'));
            return;
        }
        if (isGlmModel) {
            setAttachmentError(t('chat.imageNotSupported'));
            return;
        }

        const newAttachments: ImageAttachment[] = [];
        const errors: string[] = [];

        for (const file of inputFiles) {
            try {
                const dataUrl = await fileToDataUrl(file);
                const base64 = extractBase64FromDataUrl(dataUrl);
                const actualBytes = getBase64ByteLength(base64);

                const sizeError = validateImageByteLength(actualBytes);
                if (sizeError) {
                    errors.push(sizeError);
                    continue;
                }

                const mimeType = extractMimeTypeFromDataUrl(dataUrl) || file.type || 'image/png';
                const mimeError = validateImageMimeType(mimeType);
                if (mimeError) {
                    errors.push(mimeError);
                    continue;
                }

                const thumbnailUrl = await createThumbnailDataUrl(dataUrl, 64, 64);
                newAttachments.push({
                    id: crypto.randomUUID(),
                    dataUrl,
                    data: base64,
                    mime_type: mimeType,
                    thumbnailUrl,
                    name: file.name || 'clipboard-image.png',
                    size: actualBytes,
                });
            } catch {
                errors.push(t('chat.imageReadFailed'));
            }
        }

        if (newAttachments.length > 0) {
            setAttachments((prev) => [...prev, ...newAttachments]);
            setAttachmentError(null);
            return;
        }

        if (errors.length > 0) {
            setAttachmentError(errors[0]);
        }
    }, [isLocalOnly, isGlmModel, t]);

    const handlePaste = useCallback(async (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
        const items = Array.from(event.clipboardData.items);
        const filesFromItems = items
            .filter((item) => item.kind === 'file')
            .map((item) => item.getAsFile())
            .filter((file): file is File => !!file);
        const filesFromClipboard = Array.from(event.clipboardData.files);

        const dedupeKey = (file: File) => `${file.name}:${file.size}:${file.lastModified}:${file.type}`;
        const fileMap = new Map<string, File>();

        const looksLikeImageFile = (file: File) => (
            file.type.startsWith('image/')
            || file.name.toLowerCase().endsWith('.png')
            || file.name.toLowerCase().endsWith('.jpg')
            || file.name.toLowerCase().endsWith('.jpeg')
            || file.name.toLowerCase().endsWith('.webp')
            || file.name.toLowerCase().endsWith('.gif')
            || file.type === ''
        );

        for (const file of [...filesFromItems, ...filesFromClipboard]) {
            if (looksLikeImageFile(file)) {
                fileMap.set(dedupeKey(file), file);
            }
        }

        // Primary path: use DataTransfer from paste event.
        if (fileMap.size > 0) {
            event.preventDefault();
            await appendImageFiles(Array.from(fileMap.values()));
            return;
        }

        // Fallback path: some WebView environments expose no files in paste
        // DataTransfer, but Clipboard API read() still returns image blobs.
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
                await appendImageFiles(clipboardFiles);
            }
        } catch {
            // Ignore Clipboard API errors and preserve default text paste behavior.
        }
    }, [appendImageFiles]);

    const handleRemoveAttachment = useCallback((id: string) => {
        setAttachments((prev) => prev.filter((attachment) => attachment.id !== id));
    }, []);

    const hasCommand = text.includes('@');

    const addCaptureAttachment = useCallback(async (result: CaptureResult, name?: string) => {
        const mimeError = validateImageMimeType(result.mime_type);
        if (mimeError) {
            setAttachmentError(mimeError);
            return;
        }
        const byteLength = getBase64ByteLength(result.data);
        const sizeError = validateImageByteLength(byteLength);
        if (sizeError) {
            setAttachmentError(sizeError);
            return;
        }
        const dataUrl = `data:${result.mime_type};base64,${result.data}`;
        const thumbnailUrl = await createThumbnailDataUrl(dataUrl, 64, 64);
        setAttachments((prev) => [
            ...prev,
            {
                id: crypto.randomUUID(),
                dataUrl,
                data: result.data,
                mime_type: result.mime_type,
                thumbnailUrl,
                name: name || 'screenshot.png',
                size: byteLength,
            },
        ]);
        setAttachmentError(null);
    }, []);

    const handleWindowCapture = useCallback(async () => {
        if (isLocalOnly) {
            setAttachmentError(t('chat.imageNoSubscription'));
            return;
        }
        if (isGlmModel) {
            setAttachmentError(t('chat.imageNotSupported'));
            return;
        }
        setWindowPickerMode('capture');
        setWindowPickerOpen(true);
        setWindowPickerLoading(true);
        try {
            const windows = await invoke<WindowInfo[]>('list_capturable_windows');
            setCapturableWindows(windows);
        } catch (error) {
            console.error('[CommandCenter] Failed to list windows:', error);
            setAttachmentError('Failed to list windows for capture.');
            setWindowPickerOpen(false);
        } finally {
            setWindowPickerLoading(false);
        }
    }, [isLocalOnly, isGlmModel]);

    const handleRegionCapture = useCallback(async () => {
        if (isLocalOnly) {
            setAttachmentError(t('chat.imageNoSubscription'));
            return;
        }
        if (isGlmModel) {
            setAttachmentError(t('chat.imageNotSupported'));
            return;
        }
        setWindowPickerMode('region');
        setWindowPickerOpen(true);
        setWindowPickerLoading(true);
        try {
            const windows = await invoke<WindowInfo[]>('list_capturable_windows');
            setCapturableWindows(windows);
        } catch (error) {
            console.error('[CommandCenter] Failed to list windows:', error);
            setAttachmentError('Failed to list windows for capture.');
            setWindowPickerOpen(false);
        } finally {
            setWindowPickerLoading(false);
        }
    }, [isLocalOnly, isGlmModel]);

    const handleScreenshot = useCallback((mode: 'window' | 'region') => {
        if (mode === 'window') {
            void handleWindowCapture();
        } else {
            void handleRegionCapture();
        }
    }, [handleRegionCapture, handleWindowCapture]);

    const handleUploadImage = useCallback(async () => {
        if (isLocalOnly) {
            setAttachmentError(t('chat.imageNoSubscription'));
            return;
        }
        if (isGlmModel) {
            setAttachmentError(t('chat.imageNotSupported'));
            return;
        }
        try {
            const selected = await open({
                multiple: true,
                filters: [{
                    name: 'Images',
                    extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'],
                }],
            });
            if (!selected) return;
            const paths = Array.isArray(selected) ? selected : [selected];

            const newAttachments: ImageAttachment[] = [];
            const errors: string[] = [];

            for (const filePath of paths) {
                const bytes = await readFile(filePath);
                const sizeError = validateImageByteLength(bytes.length);
                if (sizeError) {
                    errors.push(sizeError);
                    continue;
                }
                const ext = filePath.split('.').pop()?.toLowerCase() || '';
                const mimeMap: Record<string, string> = {
                    png: 'image/png',
                    jpg: 'image/jpeg',
                    jpeg: 'image/jpeg',
                    webp: 'image/webp',
                    gif: 'image/gif',
                };
                const mimeType = mimeMap[ext] || 'image/png';
                const mimeError = validateImageMimeType(mimeType);
                if (mimeError) {
                    errors.push(mimeError);
                    continue;
                }
                // Convert Uint8Array to base64
                let binary = '';
                for (let i = 0; i < bytes.length; i++) {
                    binary += String.fromCharCode(bytes[i]);
                }
                const base64 = btoa(binary);
                const dataUrl = `data:${mimeType};base64,${base64}`;
                const thumbnailUrl = await createThumbnailDataUrl(dataUrl, 64, 64);
                const fileName = filePath.split('/').pop() || filePath.split('\\').pop() || 'image';
                newAttachments.push({
                    id: crypto.randomUUID(),
                    dataUrl,
                    data: base64,
                    mime_type: mimeType,
                    thumbnailUrl,
                    name: fileName,
                    size: bytes.length,
                });
            }

            if (newAttachments.length > 0) {
                setAttachments((prev) => [...prev, ...newAttachments]);
            }
            if (errors.length > 0) {
                setAttachmentError(errors[0]);
            } else if (newAttachments.length > 0) {
                setAttachmentError(null);
            }
        } catch (error) {
            console.error('[CommandCenter] Failed to upload image:', error);
            setAttachmentError('Failed to upload image.');
        }
    }, [isLocalOnly, isGlmModel]);

    const handleWindowSelect = useCallback(async (windowId: number) => {
        setWindowPickerLoading(true);
        try {
            // Close the picker first and wait for it to disappear from screen
            // so it doesn't appear in the captured screenshot
            setWindowPickerOpen(false);
            await new Promise(resolve => setTimeout(resolve, 500));
            const result = await invoke<CaptureResult>('capture_window', { windowId });
            if (windowPickerMode === 'region') {
                const dataUrl = `data:${result.mime_type};base64,${result.data}`;
                setRegionSourceWindowId(windowId);
                setRegionCapture({ dataUrl, width: result.width, height: result.height });
            } else {
                await addCaptureAttachment(result, `window-${windowId}.png`);
            }
        } catch (error) {
            console.error('[CommandCenter] Failed to capture window:', error);
            setAttachmentError('Failed to capture window.');
        } finally {
            setWindowPickerLoading(false);
        }
    }, [addCaptureAttachment, windowPickerMode]);

    const handleRegionConfirm = useCallback(async (region: { x: number; y: number; width: number; height: number }) => {
        if (regionSourceWindowId == null) {
            setAttachmentError('No source window selected.');
            setRegionCapture(null);
            return;
        }
        setIsCapturing(true);
        try {
            const result = await invoke<CaptureResult>('capture_window_region', {
                windowId: regionSourceWindowId,
                x: region.x,
                y: region.y,
                width: region.width,
                height: region.height,
            });
            await addCaptureAttachment(result, 'region.png');
        } catch (error) {
            console.error('[CommandCenter] Failed to capture region:', error);
            setAttachmentError('Failed to capture region.');
        } finally {
            setIsCapturing(false);
            setRegionCapture(null);
            setRegionSourceWindowId(null);
        }
    }, [addCaptureAttachment, regionSourceWindowId]);

    // Detect @ and show command popup
    const handleTextChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const newText = e.target.value;

        // Update text immediately for responsive typing
        setText(newText);
        scheduleTextareaResize(e.currentTarget);

        const cursorPos = e.target.selectionStart;
        const trigger = detectComposerTrigger(newText, cursorPos);

        if (trigger?.kind === 'path') {
            setMentionQuery(trigger.query);
            setShowSuggestions(true);
            setSelectedSuggestionIndex(0);
            return;
        }

        setShowSuggestions(false);
        setMentionQuery('');
    }, [scheduleTextareaResize]);

    const insertSuggestion = useCallback((suggestion: ComposerSuggestion) => {
        const cursorPos = textareaRef.current?.selectionStart || 0;
        const trigger = detectComposerTrigger(text, cursorPos);

        if (trigger?.kind === 'path') {
            const replacement = suggestion.kind === 'command'
                ? `@${suggestion.name} `
                : `@${suggestion.path}${suggestion.isDir ? '/' : ' '}`;
            const nextState = replaceTextRange(text, trigger.rangeStart, trigger.rangeEnd, replacement);
            setText(nextState.text);
            scheduleTextareaResize(textareaRef.current);
            if (suggestion.kind === 'path') {
                setSelectedPathMentions((prev) => ({
                    ...prev,
                    [suggestion.path]: { path: suggestion.path, is_dir: suggestion.isDir },
                }));
            }
            setMentionQuery(suggestion.kind === 'path' && suggestion.isDir ? `${suggestion.path}/` : '');
            setShowSuggestions(suggestion.kind === 'path' && suggestion.isDir);

            setTimeout(() => {
                if (!textareaRef.current) return;
                textareaRef.current.focus();
                textareaRef.current.setSelectionRange(nextState.cursor, nextState.cursor);
            }, 0);
        }
    }, [scheduleTextareaResize, text]);

    const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
        // Handle command popup navigation
        if (showSuggestions && suggestions.length > 0) {
            if (e.key === 'ArrowDown') {
                e.preventDefault();
                setSelectedSuggestionIndex(prev =>
                    prev < suggestions.length - 1 ? prev + 1 : 0
                );
                return;
            }
            if (e.key === 'ArrowUp') {
                e.preventDefault();
                setSelectedSuggestionIndex(prev =>
                    prev > 0 ? prev - 1 : suggestions.length - 1
                );
                return;
            }
            if (e.key === 'Enter' || e.key === 'Tab') {
                e.preventDefault();
                insertSuggestion(suggestions[selectedSuggestionIndex]);
                return;
            }
            if (e.key === 'Escape') {
                e.preventDefault();
                setShowSuggestions(false);
                return;
            }
        }

        // Handle multiline insertion explicitly to keep caret rendering in sync
        // (prevents occasional visual cursor drift after Shift+Enter in controlled textarea).
        if (e.key === 'Enter' && e.shiftKey) {
            e.preventDefault();
            const textarea = e.currentTarget;
            const start = textarea.selectionStart;
            const end = textarea.selectionEnd;
            const nextText = `${text.slice(0, start)}\n${text.slice(end)}`;

            setText(nextText);
            setShowSuggestions(false);

            requestAnimationFrame(() => {
                if (!textareaRef.current) return;
                const nextPos = start + 1;
                textareaRef.current.focus();
                textareaRef.current.setSelectionRange(nextPos, nextPos);
                resizeTextarea(textareaRef.current);
            });
            return;
        }

        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            if (isLocalOnly && attachments.length > 0) {
                setAttachmentError(t('chat.imageNoSubscription'));
                return;
            }
            if (isGlmModel && attachments.length > 0) {
                setAttachmentError(t('chat.imageNotSupported'));
                return;
            }
            if ((text.trim() || attachments.length > 0) && !disabled) {
                const mentions = collectActivePathMentions(text, Object.values(selectedPathMentions));
                onSend(text, attachments, mentions, chatMode);
                setText('');
                setAttachments([]);
                setSelectedPathMentions({});
                setAttachmentError(null);
                setShowSuggestions(false);
                setMentionQuery('');
                scheduleTextareaResize(textareaRef.current);
            }
        }
    };

    return (
        <>
            <div className="shrink-0 border-t border-[var(--border-subtle)] bg-[var(--bg-app)]/95 backdrop-blur-md">
                <div className="px-2 pb-2 pt-2">
                    <div className={`relative rounded-xl border shadow-[0_16px_44px_rgba(0,0,0,0.22)] transition-colors ${chatMode === 'planning'
                        ? 'border-sky-500/35 bg-[var(--bg-surface)]/85'
                        : 'border-[var(--border-subtle)] bg-[var(--bg-surface)]/85'
                        }`}>
                    <div className={`relative z-[70] border-b px-2 py-1.5 transition-colors ${chatMode === 'planning'
                        ? 'border-sky-500/25'
                        : 'border-[var(--border-subtle)]/50'
                        }`}>
                        <div className="flex items-center justify-between gap-2">
                            <div className="flex items-center gap-1.5">
                                <FeatureMenu onScreenshot={handleScreenshot} onUploadImage={handleUploadImage} disabled={disabled} />
                                <div className={`inline-flex items-center gap-1 rounded-lg border p-1 ${chatMode === 'planning'
                                    ? 'border-sky-500/25 bg-[var(--bg-app)]/70'
                                    : 'border-[var(--border-subtle)] bg-[var(--bg-app)]/70'
                                    }`}>
                                    {chatModeOptions.map((option) => {
                                        const Icon = option.icon;
                                        const isActive = option.value === chatMode;
                                        return (
                                            <button
                                                key={option.value}
                                                type="button"
                                                onClick={() => setChatMode(option.value)}
                                                disabled={disabled}
                                                title={option.description}
                                                className={`inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[10px] font-semibold transition-colors ${isActive
                                                    ? chatMode === 'planning'
                                                        ? 'bg-sky-500/20 text-sky-100'
                                                        : 'bg-[var(--accent-primary)]/16 text-[var(--fg-primary)]'
                                                    : 'text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)]'
                                                    }`}
                                            >
                                                <Icon className="h-3 w-3" />
                                                <span>{option.label}</span>
                                            </button>
                                        );
                                    })}
                                </div>
                            </div>
                            <div className="w-[164px] max-w-[46%] shrink-0">
                                <CompactModelSelector
                                    models={models}
                                    selectedId={selectedModelId || ''}
                                    onSelect={setSelectedModelId}
                                    disabled={disabled}
                                />
                            </div>
                        </div>
                    </div>

                    <ImageAttachmentBar attachments={attachments} onRemove={handleRemoveAttachment} />
                    {attachmentError && (
                        <div className="px-2 pb-1 pt-1 text-[10px] text-red-400">
                            {attachmentError}
                        </div>
                    )}
                    {activePathMentions.length > 0 && (
                        <div className="px-2 pb-1">
                            <div className="flex flex-wrap gap-1.5">
                                {activePathMentions.map((mention) => (
                                    <div
                                        key={mention.path}
                                        className="inline-flex max-w-full items-center gap-1.5 rounded-md border border-emerald-500/20 bg-emerald-500/8 px-2 py-1 text-[10px] text-emerald-300"
                                        title={mention.path}
                                    >
                                        {mention.is_dir ? (
                                            <Folder className="h-3 w-3 shrink-0" />
                                        ) : (
                                            <FileText className="h-3 w-3 shrink-0" />
                                        )}
                                        <span className="font-medium uppercase tracking-[0.12em] text-emerald-400/80">
                                            {t('chat.pathRef')}
                                        </span>
                                        <span className="truncate text-zinc-200">
                                            {mention.path}
                                        </span>
                                    </div>
                                ))}
                            </div>
                        </div>
                    )}

                    <div className={`relative px-1.5 pb-0 pt-1.5 transition-colors ${loading ? 'bg-[var(--bg-surface)]/40' : ''}`}>
                        {showSuggestions && suggestions.length > 0 && (
                            <div
                                className="absolute bottom-full left-0 right-0 z-[80] mx-0.5 mb-1.5 overflow-hidden rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] shadow-[0_16px_44px_rgba(0,0,0,0.34)] backdrop-blur-md"
                            >
                                {suggestions.map((suggestion, idx) => {
                                    const isActiveSuggestion = idx === selectedSuggestionIndex;
                                    if (suggestion.kind === 'command') {
                                        const Icon = suggestion.icon;
                                        return (
                                            <button
                                                key={suggestion.key}
                                                onClick={() => insertSuggestion(suggestion)}
                                                title={suggestion.tooltip}
                                                className={`w-full flex items-center gap-2 px-2 py-1.5 text-left transition-colors ${isActiveSuggestion
                                                    ? 'bg-[var(--accent-primary)]/15 text-[var(--fg-primary)]'
                                                    : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)]'
                                                    }`}
                                            >
                                                <div className="flex h-6 w-6 items-center justify-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                                                    <Icon className="h-3 w-3 text-[var(--accent-primary)]" />
                                                </div>
                                                <div className="min-w-0 flex-1">
                                                    <span className="text-[10px] font-semibold">@{suggestion.name}</span>
                                                    <span className="ml-1 text-[9px] text-[var(--fg-tertiary)]">{suggestion.description}</span>
                                                </div>
                                            </button>
                                        );
                                    }

                                    return (
                                        <button
                                            key={suggestion.key}
                                            onClick={() => insertSuggestion(suggestion)}
                                            className={`w-full flex items-center gap-2 px-2 py-1.5 text-left transition-colors ${isActiveSuggestion
                                                ? 'bg-[var(--accent-primary)]/15 text-[var(--fg-primary)]'
                                                : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)]'
                                                }`}
                                            title={suggestion.path}
                                        >
                                            <div className="flex h-6 w-6 items-center justify-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                                                {suggestion.isDir ? (
                                                    <Folder className="h-3 w-3 text-[var(--accent-primary)]" />
                                                ) : (
                                                    <FileText className="h-3 w-3 text-[var(--accent-primary)]" />
                                                )}
                                            </div>
                                            <div className="min-w-0 flex-1">
                                                <span className="block truncate text-[10px] font-semibold">@{suggestion.path}</span>
                                                <span className="text-[9px] text-[var(--fg-tertiary)]">{suggestion.isDir ? t('chat.pathType.folder') : t('chat.pathType.file')}</span>
                                            </div>
                                        </button>
                                    );
                                })}
                            </div>
                        )}
                        <div className={`relative overflow-hidden rounded-lg border bg-[var(--bg-app)] transition-colors shadow-[inset_0_1px_0_rgba(255,255,255,0.03)] ${chatMode === 'planning'
                            ? loading
                                ? 'border-sky-400/40'
                                : 'border-sky-500/25 focus-within:border-sky-400/50'
                            : loading
                                ? 'border-[var(--border-focus)] bg-[var(--bg-surface)]/70'
                                : 'border-[var(--border-subtle)] focus-within:border-[var(--accent-primary)]/60'
                            }`}>
                            <textarea
                                ref={textareaRef}
                                value={text}
                                onChange={handleTextChange}
                                onPaste={handlePaste}
                                onKeyDown={handleKeyDown}
                                placeholder={chatMode === 'planning' ? `${t('chat.planningPlaceholder')} (Ctrl-L to focus)` : `${t('chat.inputPlaceholder')} (Ctrl-L to focus)`}
                                className="relative z-10 min-h-[88px] max-h-[360px] w-full resize-none overflow-y-auto bg-transparent px-3 pb-3 pt-2.5 pr-14 text-[13px] font-medium leading-[18px] text-[var(--fg-primary)] outline-none placeholder-[var(--fg-tertiary)]"
                                rows={1}
                                disabled={disabled}
                            />
                            <button
                                onClick={() => {
                                    const showStop = loading && !text.trim();
                                    if (showStop && onStop) {
                                        onStop();
                                    } else if (isLocalOnly && attachments.length > 0) {
                                        setAttachmentError(t('chat.imageNoSubscription'));
                                    } else if (isGlmModel && attachments.length > 0) {
                                        setAttachmentError(t('chat.imageNotSupported'));
                                    } else if ((text.trim() || attachments.length > 0) && !disabled) {
                                        const mentions = collectActivePathMentions(text, Object.values(selectedPathMentions));
                                        onSend(text, attachments, mentions, chatMode);
                                        setText('');
                                        setAttachments([]);
                                        setSelectedPathMentions({});
                                        setAttachmentError(null);
                                        setShowSuggestions(false);
                                        setMentionQuery('');
                                        scheduleTextareaResize(textareaRef.current);
                                    }
                                }}
                                disabled={(!text.trim() && attachments.length === 0 && !loading) || disabled}
                                className={`absolute bottom-2 right-2 z-20 inline-flex h-9 w-9 items-center justify-center rounded-lg border transition-colors ${loading && !text.trim()
                                    ? 'border-red-500/30 bg-red-500/10 text-red-400 hover:bg-red-500/20'
                                    : chatMode === 'planning'
                                        ? 'border-sky-500/25 bg-sky-500/10 text-sky-100 hover:border-sky-400/40 hover:bg-sky-500/15 disabled:cursor-not-allowed disabled:opacity-30'
                                        : 'border-[var(--border-subtle)] bg-[var(--bg-surface)] text-[var(--fg-tertiary)] hover:border-[var(--accent-primary)]/40 hover:text-[var(--fg-primary)] disabled:cursor-not-allowed disabled:opacity-30'
                                    }`}
                            >
                                {loading && !text.trim() ? (
                                    <Square className="h-4 w-4 fill-current animate-pulse" />
                                ) : (
                                    <Send className="h-4 w-4" />
                                )}
                            </button>
                        </div>
                        <div className="flex items-center justify-between border-t border-[var(--border-subtle)]/40 px-2 py-1 text-[10px] text-[var(--fg-tertiary)]">
                            <span>{chatMode === 'planning' ? t('chat.composer.planningReadonly') : t('chat.composer.enterToSend')}</span>
                            <span>{chatMode === 'planning' ? t('chat.composer.planningNoEdits') : t('chat.composer.shiftEnterNewline')}</span>
                        </div>
                    </div>
                </div>
            </div>
            </div>
            <WindowPicker
                isOpen={windowPickerOpen}
                windows={capturableWindows}
                loading={windowPickerLoading}
                title={windowPickerMode === 'region' ? t('screenshot.regionPickerTitle') : t('screenshot.windowPickerTitle')}
                subtitle={windowPickerMode === 'region' ? t('screenshot.regionPickerSubtitle') : t('screenshot.windowPickerSubtitle')}
                onSelect={handleWindowSelect}
                onCancel={() => setWindowPickerOpen(false)}
            />
            {regionCapture && (
                <RegionSelector
                    isOpen={Boolean(regionCapture)}
                    dataUrl={regionCapture.dataUrl}
                    imageWidth={regionCapture.width}
                    imageHeight={regionCapture.height}
                    onCancel={() => setRegionCapture(null)}
                    onConfirm={handleRegionConfirm}
                />
            )}
        </>
    );
};

// Custom comparison - only re-render when props that affect CommandCenter actually change
export const CommandCenter = React.memo(CommandCenterComponent, (prevProps, nextProps) => {
    // Check primitive props
    if (prevProps.disabled !== nextProps.disabled) return false;
    if (prevProps.loading !== nextProps.loading) return false;
    if (prevProps.selectedModelId !== nextProps.selectedModelId) return false;
    if (prevProps.chatMode !== nextProps.chatMode) return false;
    
    // Check callback references (should be stable with useCallback in parent)
    if (prevProps.onSend !== nextProps.onSend) return false;
    if (prevProps.onStop !== nextProps.onStop) return false;
    if (prevProps.setSelectedModelId !== nextProps.setSelectedModelId) return false;
    if (prevProps.setChatMode !== nextProps.setChatMode) return false;
    if (prevProps.onPrefillConsumed !== nextProps.onPrefillConsumed) return false;

    // Prefill state (queue item edit)
    const prevPrefill = prevProps.prefillRequest;
    const nextPrefill = nextProps.prefillRequest;
    if ((prevPrefill?.text ?? '') !== (nextPrefill?.text ?? '')) return false;
    if ((prevPrefill?.attachments?.length ?? 0) !== (nextPrefill?.attachments?.length ?? 0)) return false;
    if ((prevPrefill?.mentions?.length ?? 0) !== (nextPrefill?.mentions?.length ?? 0)) return false;
    if ((prevPrefill?.mode ?? 'code') !== (nextPrefill?.mode ?? 'code')) return false;

    if (prevProps.models.length !== nextProps.models.length) return false;
    for (let i = 0; i < prevProps.models.length; i++) {
        if (prevProps.models[i].id !== nextProps.models[i].id) return false;
    }
    
    return true;
});
