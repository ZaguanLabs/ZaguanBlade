import React, { useCallback, useMemo, useRef, useState } from 'react';
import type { CaptureResult } from '../../types/screenshot';
import type { ChatMode, ComposerMention, ModelInfo, QueuedRequest } from '../../types/chat';
import { ComposerTextarea, type ComposerTextareaHandle, replaceActiveTrigger } from './ComposerTextarea';
import { ComposerToolbar } from './ComposerToolbar';
import { MentionSuggestions, type ComposerSuggestion } from './MentionSuggestions';
import { useComposerAttachments } from './useComposerAttachments';
import { usePathSuggestions, type WorkspacePathMatch } from './usePathSuggestions';

interface ComposerProps {
    onSend: (text: string, attachments?: ReturnType<typeof useComposerAttachments>['attachments'], mentions?: ComposerMention[], mode?: ChatMode) => void;
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

function collectMentions(text: string, selected: Record<string, WorkspacePathMatch>): ComposerMention[] {
    return Object.values(selected)
        .filter((entry) => text.includes(`@${entry.path}`))
        .map((entry) => ({ kind: 'path' as const, path: entry.path, is_dir: entry.is_dir }));
}

export const Composer: React.FC<ComposerProps> = ({
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
    const [text, setText] = useState('');
    const [mentionQuery, setMentionQuery] = useState<string | null>(null);
    const [selectedSuggestionIndex] = useState(0);
    const [selectedMentions, setSelectedMentions] = useState<Record<string, WorkspacePathMatch>>({});
    const textareaRef = useRef<ComposerTextareaHandle>(null);
    const textareaDomCursorRef = useRef(0);
    const isLocalOnly = models.length > 0 && models.every((model) => model.provider === 'ollama' || model.provider === 'openai-compat');
    const isGlmModel = selectedModelId.toLowerCase().includes('glm-4') || selectedModelId.toLowerCase().includes('glm-5');
    const imageDisabledReason = isLocalOnly ? 'Images require a subscription model.' : isGlmModel ? 'Images are not supported by this model.' : undefined;
    const attachments = useComposerAttachments({
        disabledImages: isLocalOnly || isGlmModel,
        disabledReason: imageDisabledReason,
    });
    const pathSuggestions = usePathSuggestions(mentionQuery ?? '', mentionQuery !== null);
    const suggestions = useMemo<ComposerSuggestion[]>(() => {
        if (mentionQuery === null) {
            return [];
        }
        const commands: ComposerSuggestion[] = (['web', 'research'] as const)
            .filter((name) => name.startsWith(mentionQuery.toLowerCase()))
            .map((name) => ({ kind: 'command', key: `command:${name}`, name }));
        return [
            ...commands,
            ...pathSuggestions.map((entry) => ({ kind: 'path' as const, key: `path:${entry.path}`, entry })),
        ];
    }, [mentionQuery, pathSuggestions]);

    const submit = useCallback(() => {
        if (disabled) {
            return;
        }
        if (!text.trim() && attachments.attachments.length === 0) {
            return;
        }
        onSend(text, attachments.attachments, collectMentions(text, selectedMentions), chatMode);
        setText('');
        setMentionQuery(null);
        setSelectedMentions({});
        attachments.clearAttachments();
        requestAnimationFrame(() => textareaRef.current?.resize());
    }, [attachments, chatMode, disabled, onSend, selectedMentions, text]);

    const selectSuggestion = useCallback((suggestion: ComposerSuggestion) => {
        const textarea = document.activeElement instanceof HTMLTextAreaElement ? document.activeElement : null;
        const cursor = textarea?.selectionStart ?? textareaDomCursorRef.current ?? text.length;
        const entry = suggestion.kind === 'path'
            ? suggestion.entry
            : { path: suggestion.name, is_dir: false };
        const next = replaceActiveTrigger(text, cursor, entry);
        setText(next.text);
        if (suggestion.kind === 'path') {
            setSelectedMentions((current) => ({
                ...current,
                [suggestion.entry.path]: suggestion.entry,
            }));
        }
        setMentionQuery(suggestion.kind === 'path' && suggestion.entry.is_dir ? suggestion.entry.path : null);
        requestAnimationFrame(() => textareaRef.current?.focus());
    }, [text]);

    const capture = useCallback((result: CaptureResult, name: string) => {
        void attachments.appendCapture(result, name);
    }, [attachments]);

    React.useEffect(() => {
        if (!prefillRequest) {
            return;
        }
        setText(prefillRequest.text);
        setChatMode(prefillRequest.mode);
        setSelectedMentions(Object.fromEntries((prefillRequest.mentions || []).map((mention) => [
            mention.path,
            { path: mention.path, is_dir: mention.is_dir },
        ])));
        onPrefillConsumed?.();
        requestAnimationFrame(() => textareaRef.current?.focus());
    }, [onPrefillConsumed, prefillRequest, setChatMode]);

    const activeMentionChips = useMemo(() => collectMentions(text, selectedMentions), [selectedMentions, text]);

    return (
        <div className="shrink-0 border-t border-(--border-subtle) bg-(--bg-panel)">
            <div className="px-2 pb-2 pt-2">
                <div className={`relative rounded-[calc(var(--panel-radius)*0.9)] border bg-(--bg-surface)/85 shadow-(--shadow-md) ${chatMode === 'planning' ? 'border-[color-mix(in_srgb,var(--accent-planning)_42%,transparent)]' : 'border-(--border-subtle)'}`}>
                    <ComposerToolbar
                        loading={loading}
                        disabled={disabled}
                        canSend={text.trim().length > 0 || attachments.attachments.length > 0}
                        chatMode={chatMode}
                        setChatMode={setChatMode}
                        models={models}
                        selectedModelId={selectedModelId}
                        setSelectedModelId={setSelectedModelId}
                        attachments={attachments.attachments}
                        attachmentError={attachments.attachmentError}
                        onUploadImage={attachments.uploadImages}
                        onRemoveAttachment={attachments.removeAttachment}
                        onCapture={capture}
                        onAttachmentError={attachments.setAttachmentError}
                        onSubmit={submit}
                        onStop={onStop}
                    />
                    {activeMentionChips.length > 0 && (
                        <div className="flex flex-wrap gap-1.5 px-2 pb-1">
                            {activeMentionChips.map((mention) => (
                                <span key={mention.path} className="inline-flex max-w-full rounded-md border border-[color-mix(in_srgb,var(--accent-mention)_24%,transparent)] bg-[color-mix(in_srgb,var(--accent-mention)_10%,transparent)] px-2 py-1 text-[10px] text-(--accent-mention)">
                                    @{mention.path}
                                </span>
                            ))}
                        </div>
                    )}
                    <div className="relative px-1.5 pb-0 pt-1.5">
                        <MentionSuggestions
                            suggestions={suggestions}
                            selectedIndex={selectedSuggestionIndex}
                            onSelect={selectSuggestion}
                        />
                        <div className="relative overflow-hidden rounded-lg border border-(--border-subtle) bg-(--bg-app)">
                            <ComposerTextarea
                                ref={textareaRef}
                                text={text}
                                setText={(value) => {
                                    setText(value);
                                    textareaDomCursorRef.current = value.length;
                                }}
                                disabled={disabled}
                                placeholder={chatMode === 'planning' ? 'Plan a change...' : 'Ask or request a change...'}
                                showSuggestions={mentionQuery !== null}
                                suggestions={suggestions}
                                selectedSuggestionIndex={selectedSuggestionIndex}
                                onTriggerChange={setMentionQuery}
                                onSelectSuggestion={selectSuggestion}
                                onSubmit={submit}
                                onPasteImages={(files) => void attachments.appendFiles(files)}
                            />
                        </div>
                        <div className="flex items-center justify-between border-t border-(--border-subtle)/40 px-2 py-1 text-[10px] text-(--fg-tertiary)">
                            <span>Enter to send</span>
                            <span>Shift-Enter for newline</span>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
};
