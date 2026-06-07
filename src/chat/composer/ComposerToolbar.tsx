import React from 'react';
import { Code2, Map as MapIcon, Send, Square } from 'lucide-react';
import type { ChatMode, ImageAttachment, ModelInfo } from '../../types/chat';
import { AttachmentControls } from './AttachmentControls';
import { ModelSelectButton } from './ModelSelectButton';
import { ScreenshotPickerLauncher } from './ScreenshotPickerLauncher';
import type { CaptureResult } from '../../types/screenshot';

export const ComposerToolbar: React.FC<{
    loading?: boolean;
    disabled?: boolean;
    canSend: boolean;
    chatMode: ChatMode;
    setChatMode: (mode: ChatMode) => void;
    models: ModelInfo[];
    selectedModelId: string;
    setSelectedModelId: (modelId: string) => void;
    attachments: ImageAttachment[];
    attachmentError: string | null;
    imagesDisabled: boolean;
    imageDisabledReason?: string;
    onUploadImage: () => void;
    onRemoveAttachment: (id: string) => void;
    onCapture: (result: CaptureResult, name: string) => void;
    onAttachmentError: (message: string) => void;
    onSubmit: () => void;
    onStop?: () => void;
}> = ({
    loading,
    disabled,
    canSend,
    chatMode,
    setChatMode,
    models,
    selectedModelId,
    setSelectedModelId,
    attachments,
    attachmentError,
    imagesDisabled,
    imageDisabledReason,
    onUploadImage,
    onRemoveAttachment,
    onCapture,
    onAttachmentError,
    onSubmit,
    onStop,
}) => (
    <>
        <div className="flex items-center justify-between gap-2 border-b border-(--border-subtle)/60 px-2 py-1.5">
            <div className="flex items-center gap-1.5">
                <ScreenshotPickerLauncher
                    disabled={disabled}
                    imagesDisabled={imagesDisabled}
                    imageDisabledReason={imageDisabledReason}
                    onUploadImage={onUploadImage}
                    onCapture={onCapture}
                    onError={onAttachmentError}
                />
                <div className="inline-flex rounded-[calc(var(--panel-radius)*0.55)] border border-(--border-subtle) bg-(--bg-app) p-1">
                    <button type="button" onClick={() => setChatMode('code')} disabled={disabled} aria-pressed={chatMode === 'code'} className={`inline-flex items-center gap-1 rounded-[calc(var(--panel-radius)*0.35)] px-2 py-1 text-[10px] ${chatMode === 'code' ? 'bg-(--accent-ai)/15 text-(--fg-primary)' : 'text-(--fg-tertiary)'}`}>
                        <Code2 className="h-3 w-3" aria-hidden="true" />
                        Code
                    </button>
                    <button type="button" onClick={() => setChatMode('planning')} disabled={disabled} aria-pressed={chatMode === 'planning'} className={`inline-flex items-center gap-1 rounded-[calc(var(--panel-radius)*0.35)] px-2 py-1 text-[10px] ${chatMode === 'planning' ? 'bg-[color-mix(in_srgb,var(--accent-planning)_20%,transparent)] text-(--fg-primary)' : 'text-(--fg-tertiary)'}`}>
                        <MapIcon className="h-3 w-3" aria-hidden="true" />
                        Plan
                    </button>
                </div>
            </div>
            <div className="w-[164px] max-w-[46%] shrink-0">
                <ModelSelectButton
                    models={models}
                    selectedModelId={selectedModelId}
                    setSelectedModelId={setSelectedModelId}
                    disabled={disabled}
                />
            </div>
        </div>
        <AttachmentControls
            attachments={attachments}
            error={attachmentError}
            onRemove={onRemoveAttachment}
        />
        <button
            type="button"
            onClick={loading && !canSend ? onStop : onSubmit}
            disabled={disabled || (!canSend && !loading)}
            aria-label={loading && !canSend ? 'Stop response' : 'Send message'}
            className={`absolute bottom-8 right-4 z-20 inline-flex h-9 w-9 items-center justify-center rounded-[calc(var(--panel-radius)*0.55)] border transition-colors ${loading && !canSend
                ? 'border-[color-mix(in_srgb,var(--state-danger)_34%,transparent)] bg-[color-mix(in_srgb,var(--state-danger)_12%,transparent)] text-(--state-danger)'
                : 'border-(--border-subtle) bg-(--bg-surface) text-(--fg-tertiary) hover:text-(--fg-primary)'
                }`}
        >
            {loading && !canSend ? <Square className="h-4 w-4 fill-current" aria-hidden="true" /> : <Send className="h-4 w-4" aria-hidden="true" />}
        </button>
    </>
);
