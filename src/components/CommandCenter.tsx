import React, { useState, useRef, useCallback, useMemo, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { readFile } from '@tauri-apps/plugin-fs';
import { Send, Square, BookOpen, Globe } from 'lucide-react';
import { CompactModelSelector } from './CompactModelSelector';
import { FeatureMenu } from './FeatureMenu';
import { ImageAttachmentBar } from './ImageAttachmentBar';
import { WindowPicker } from './WindowPicker';
import { RegionSelector } from './RegionSelector';
import type { ImageAttachment, ModelInfo, QueuedRequest } from '../types/chat';
import type { CaptureResult, WindowInfo } from '../types/screenshot';
import {
    createThumbnailDataUrl,
    extractBase64FromDataUrl,
    extractMimeTypeFromDataUrl,
    fileToDataUrl,
    getBase64ByteLength,
    validateImageByteLength,
    validateImageMimeType,
    validateImageSize
} from '../utils/imageUtils';

const COMMANDS = [
    { 
        name: 'web', 
        description: 'Send link to model',
        tooltip: 'Fetches content from a URL and uses it as information to help the model make better decisions',
        icon: Globe 
    },
    { 
        name: 'research', 
        description: 'Research any topic',
        tooltip: 'Performs deep research on a topic and displays results in a new tab once complete',
        icon: BookOpen 
    },
];

interface CommandCenterProps {
    onSend: (text: string, attachments?: ImageAttachment[]) => void;
    onStop?: () => void;
    disabled?: boolean;
    loading?: boolean;
    models: ModelInfo[];
    selectedModelId: string;
    setSelectedModelId: (modelId: string) => void;
    prefillRequest?: QueuedRequest | null;
    onPrefillConsumed?: () => void;
}

const CommandCenterComponent: React.FC<CommandCenterProps> = ({
    onSend,
    onStop,
    disabled,
    loading,
    models,
    selectedModelId,
    setSelectedModelId,
    prefillRequest,
    onPrefillConsumed,
}) => {
    const { t } = useTranslation();
    const [text, setText] = useState('');
    const [showCommands, setShowCommands] = useState(false);
    const [selectedCommandIndex, setSelectedCommandIndex] = useState(0);
    const [commandFilter, setCommandFilter] = useState('');
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
    const isLocalOnly = useMemo(() => (
        models.length > 0
        && models.every((model) => model.provider === 'ollama' || model.provider === 'openai-compat')
    ), [models]);

    const isGlmModel = useMemo(() => {
        const id = selectedModelId.toLowerCase();
        return id.includes('glm-4') || id.includes('glm-5') || id.includes('glm4') || id.includes('glm5');
    }, [selectedModelId]);

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
        setAttachmentError(null);

        requestAnimationFrame(() => {
            if (!textareaRef.current) return;
            const end = prefillRequest.text.length;
            textareaRef.current.focus();
            textareaRef.current.setSelectionRange(end, end);
            resizeTextarea(textareaRef.current);
        });

        onPrefillConsumed?.();
    }, [prefillRequest, onPrefillConsumed, resizeTextarea]);

    const handlePaste = useCallback(async (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
        const items = Array.from(event.clipboardData.items);
        const filesFromItems = items
            .map((item) => item.getAsFile())
            .filter((file): file is File => !!file);
        const filesFromClipboard = Array.from(event.clipboardData.files);

        const dedupeKey = (file: File) => `${file.name}:${file.size}:${file.lastModified}`;
        const fileMap = new Map<string, File>();
        [...filesFromItems, ...filesFromClipboard].forEach((file) => {
            fileMap.set(dedupeKey(file), file);
        });

        const looksLikeImageFile = (file: File) => {
            if (file.type.startsWith('image/')) return true;
            const lowerName = file.name.toLowerCase();
            return lowerName.endsWith('.png')
                || lowerName.endsWith('.jpg')
                || lowerName.endsWith('.jpeg')
                || lowerName.endsWith('.webp')
                || lowerName.endsWith('.gif')
                || file.type === '';
        };

        const pastedFiles = Array.from(fileMap.values()).filter(looksLikeImageFile);
        if (pastedFiles.length === 0) return;

        event.preventDefault();

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
        for (const file of pastedFiles) {
            const sizeError = validateImageSize(file);
            if (sizeError) {
                errors.push(sizeError);
                continue;
            }

            const dataUrl = await fileToDataUrl(file);
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
                data: extractBase64FromDataUrl(dataUrl),
                mime_type: mimeType,
                thumbnailUrl,
                name: file.name,
                size: file.size,
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
    }, [isLocalOnly, isGlmModel]);

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

    // Filter commands based on what user typed after @
    const filteredCommands = React.useMemo(() =>
        COMMANDS.filter(cmd =>
            cmd.name.toLowerCase().startsWith(commandFilter.toLowerCase())
        ),
        [commandFilter]);

    // Detect @ and show command popup
    const handleTextChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const newText = e.target.value;

        // Update text immediately for responsive typing
        setText(newText);
        scheduleTextareaResize(e.currentTarget);

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
    }, [scheduleTextareaResize]);

    // Insert selected command
    const insertCommand = useCallback((commandName: string) => {
        const cursorPos = textareaRef.current?.selectionStart || 0;
        const textBeforeCursor = text.slice(0, cursorPos);
        const textAfterCursor = text.slice(cursorPos);
        const lastAtIndex = textBeforeCursor.lastIndexOf('@');

        if (lastAtIndex !== -1) {
            const newText = textBeforeCursor.slice(0, lastAtIndex) + `@${commandName} ` + textAfterCursor;
            setText(newText);
            scheduleTextareaResize(textareaRef.current);
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
    }, [scheduleTextareaResize, text]);

    const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
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

        // Handle multiline insertion explicitly to keep caret rendering in sync
        // (prevents occasional visual cursor drift after Shift+Enter in controlled textarea).
        if (e.key === 'Enter' && e.shiftKey) {
            e.preventDefault();
            const textarea = e.currentTarget;
            const start = textarea.selectionStart;
            const end = textarea.selectionEnd;
            const nextText = `${text.slice(0, start)}\n${text.slice(end)}`;

            setText(nextText);
            setShowCommands(false);

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
                onSend(text, attachments);
                setText('');
                setAttachments([]);
                setAttachmentError(null);
                scheduleTextareaResize(textareaRef.current);
            }
        }
    };

    return (
        <>
            <div className="shrink-0 border-t border-[var(--border-subtle)] bg-[var(--bg-app)]">
                <div className="px-2 pt-3 pb-2">
                    <div className="glass-panel rounded-md shadow-[var(--shadow-lg)]">
                    {/* Header */}
                    <div className="border-b border-[var(--border-subtle)]/50 px-2 py-1">
                        <div className="flex items-center justify-between gap-2">
                            <FeatureMenu onScreenshot={handleScreenshot} onUploadImage={handleUploadImage} disabled={disabled} />
                            <div className="flex-1" />
                            <div className="w-[170px] max-w-[45%] shrink-0">
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
                        <div className="px-3 pb-2 text-[11px] text-red-400">
                            {attachmentError}
                        </div>
                    )}

                    {/* Chat Input */}
                    <div className={`relative transition-colors ${loading ? 'bg-[var(--bg-surface)]' : ''}`}>
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
                                            title={cmd.tooltip}
                                            className={`w-full flex items-center gap-3 px-3 py-2 text-left transition-colors ${idx === selectedCommandIndex
                                                ? 'bg-[var(--accent-primary)]/15 text-[var(--fg-primary)]'
                                                : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)]'
                                                }`}
                                        >
                                            <Icon className="w-4 h-4 text-[var(--accent-primary)]" />
                                            <div className="flex-1">
                                                <span className="text-xs font-semibold">@{cmd.name}</span>
                                                <span className="text-xs text-[var(--fg-tertiary)] ml-2">- {cmd.description}</span>
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
                            onPaste={handlePaste}
                            onKeyDown={handleKeyDown}
                            placeholder={t('chat.inputPlaceholder')}
                            className="w-full bg-transparent p-3 pr-12 outline-none resize-none min-h-[42px] max-h-[400px] overflow-y-auto text-xs font-sans font-semibold placeholder-[var(--fg-tertiary)] leading-relaxed relative z-10 text-[var(--fg-secondary)]"
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
                                    onSend(text, attachments);
                                    setText('');
                                    setAttachments([]);
                                    setAttachmentError(null);
                                    scheduleTextareaResize(textareaRef.current);
                                }
                            }}
                            disabled={(!text.trim() && attachments.length === 0 && !loading) || disabled}
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
    
    // Check callback references (should be stable with useCallback in parent)
    if (prevProps.onSend !== nextProps.onSend) return false;
    if (prevProps.onStop !== nextProps.onStop) return false;
    if (prevProps.setSelectedModelId !== nextProps.setSelectedModelId) return false;
    if (prevProps.onPrefillConsumed !== nextProps.onPrefillConsumed) return false;

    // Prefill state (queue item edit)
    const prevPrefill = prevProps.prefillRequest;
    const nextPrefill = nextProps.prefillRequest;
    if ((prevPrefill?.text ?? '') !== (nextPrefill?.text ?? '')) return false;
    if ((prevPrefill?.attachments?.length ?? 0) !== (nextPrefill?.attachments?.length ?? 0)) return false;
    
    // Check models array - compare by length and IDs
    if (prevProps.models.length !== nextProps.models.length) return false;
    for (let i = 0; i < prevProps.models.length; i++) {
        if (prevProps.models[i].id !== nextProps.models[i].id) return false;
    }
    
    return true;
});
