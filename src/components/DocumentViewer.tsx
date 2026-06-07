'use client';
import React, { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Save, X, FileText } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { vscDarkPlus } from 'react-syntax-highlighter/dist/esm/styles/prism';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { ScrollArea } from './ui/ScrollArea';

interface DocumentViewerProps {
  documentId: string;
  title: string;
  content: string;
  isEphemeral: boolean;
  suggestedName?: string;
  onClose: () => void;
  onSave?: (path: string) => void;
  onImplementPlan?: (planText: string) => void;

}

export const DocumentViewer: React.FC<DocumentViewerProps> = ({
  documentId,
  title,
  content,
  isEphemeral,
  suggestedName,
  onClose,
  onSave,
  onImplementPlan,

}) => {
  const { t } = useTranslation();
  const [isSaving, setIsSaving] = useState(false);
  const trimmedContent = content.trim();
  const sourceName = `${suggestedName || ''} ${title}`.toLowerCase();
  const looksLikePlanDocument = sourceName.includes('plan') || /^\s*#{1,6}\s*plan\b/im.test(content);
  const canImplementPlan = Boolean(onImplementPlan && trimmedContent && looksLikePlanDocument);

  const handleSave = async () => {
    if (!isEphemeral) return;

    console.debug('[DocumentViewer] Starting save process for:', documentId);

    setIsSaving(true);
    try {
      // Save directly to project root without dialog
      const filename = suggestedName || 'document.md';
      console.debug('[DocumentViewer] Saving document to project root:', filename);
      
      const savedPath = await invoke<string>('save_ephemeral_document_to_workspace', {
        id: documentId,
        filename: filename
      });

      console.debug('[DocumentViewer] Document saved successfully to:', savedPath);
      if (onSave) {
        onSave(savedPath);
      }
    } catch (error) {
      console.error('[DocumentViewer] Failed to save document:', error);
    } finally {
      setIsSaving(false);
    }
  };

  const handleClose = async () => {
    if (isEphemeral) {
      // Close ephemeral document (removes from memory)
      try {
        await invoke('close_ephemeral_document', { id: documentId });
      } catch (error) {
        console.error('Failed to close ephemeral document:', error);
      }
    }
    onClose();
  };

  return (
    <div className="flex flex-col h-full bg-(--bg-editor)">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-(--border-subtle) bg-(--bg-panel)/60">
        <div className="flex items-center gap-2">
          <FileText aria-hidden="true" className="w-4 h-4 text-(--accent-mention)" />
          <span className="text-sm font-medium text-(--fg-primary)">{title}</span>
          {isEphemeral && (
            <span className="text-xs px-2 py-0.5 rounded-[calc(var(--panel-radius)*0.35)] bg-[color-mix(in_srgb,var(--accent-warning)_10%,transparent)] text-(--accent-warning) border border-(--accent-warning)/20">
              Unsaved
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          {canImplementPlan && (
            <button
              type="button"
              onClick={() => onImplementPlan?.(trimmedContent)}
              className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-[calc(var(--panel-radius)*0.45)] bg-[color-mix(in_srgb,var(--accent-mention)_10%,transparent)] text-(--accent-mention) hover:bg-[color-mix(in_srgb,var(--accent-mention)_18%,transparent)] border border-(--accent-mention)/25 transition-colors"
            >
              {t('chat.implement')}
            </button>
          )}
          {isEphemeral && (
            <button
              type="button"
              onClick={handleSave}
              disabled={isSaving}
              className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-[calc(var(--panel-radius)*0.45)] bg-[color-mix(in_srgb,var(--accent-mention)_10%,transparent)] text-(--accent-mention) hover:bg-[color-mix(in_srgb,var(--accent-mention)_18%,transparent)] border border-(--accent-mention)/20 transition-colors disabled:opacity-50"
            >
              <Save aria-hidden="true" className="w-3.5 h-3.5" />
              {isSaving ? t('statusBar.saving') : t('common.save')}
            </button>
          )}
          <button
            type="button"
            onClick={handleClose}
            aria-label={t('common.close')}
            className="p-1.5 rounded-[calc(var(--panel-radius)*0.4)] hover:bg-(--bg-surface-hover) text-(--fg-tertiary) hover:text-(--fg-primary) transition-colors"
          >
            <X aria-hidden="true" className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Content */}
      <ScrollArea className="flex-1 overflow-auto">
        <div className="max-w-4xl mx-auto px-6 py-8">
          <div className="prose prose-invert prose-lg max-w-none
            prose-headings:text-(--fg-bright) prose-headings:font-semibold prose-headings:tracking-tight
            prose-h1:text-3xl prose-h1:mb-4 prose-h1:mt-0
            prose-h2:text-2xl prose-h2:mb-3 prose-h2:mt-8
            prose-h3:text-xl prose-h3:mb-2 prose-h3:mt-6
            prose-p:text-(--fg-secondary) prose-p:leading-relaxed prose-p:mb-4
            prose-strong:text-(--fg-bright) prose-strong:font-semibold
            prose-a:text-(--accent-mention) prose-a:no-underline hover:prose-a:underline
            prose-code:text-(--accent-mention) prose-code:bg-(--bg-surface) prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded-[calc(var(--panel-radius)*0.35)] prose-code:text-sm prose-code:before:content-[''] prose-code:after:content-['']
            prose-pre:bg-(--bg-surface) prose-pre:border prose-pre:border-(--border-subtle) prose-pre:rounded-[calc(var(--panel-radius)*0.75)] prose-pre:shadow-(--shadow-lg)
            prose-ul:text-(--fg-secondary) prose-ul:my-4
            prose-ol:text-(--fg-secondary) prose-ol:my-4
            prose-li:text-(--fg-secondary) prose-li:leading-relaxed prose-li:my-1
            prose-blockquote:border-l-(--accent-mention) prose-blockquote:text-(--fg-tertiary) prose-blockquote:italic
            prose-hr:border-(--border-subtle) prose-hr:my-8
            prose-table:border-collapse prose-table:my-6
            prose-thead:border-b-2 prose-thead:border-(--border-default)
            prose-th:text-(--fg-primary) prose-th:font-semibold prose-th:text-left prose-th:px-4 prose-th:py-3 prose-th:bg-(--bg-panel)/50
            prose-td:text-(--fg-secondary) prose-td:px-4 prose-td:py-3 prose-td:border-t prose-td:border-(--border-subtle)
            prose-tr:transition-colors hover:prose-tr:bg-(--bg-surface)/30">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                code({ node, inline, className, children, ...props }: any) {
                  const match = /language-(\w+)/.exec(className || '');
                  return !inline && match ? (
                    <SyntaxHighlighter
                      style={vscDarkPlus}
                      language={match[1]}
                      PreTag="div"
                      {...props}
                    >
                      {String(children).replace(/\n$/, '')}
                    </SyntaxHighlighter>
                  ) : (
                    <code className={className} {...props}>
                      {children}
                    </code>
                  );
                },
              }}
            >
              {content}
            </ReactMarkdown>
          </div>
        </div>
      </ScrollArea>
    </div>
  );
};
