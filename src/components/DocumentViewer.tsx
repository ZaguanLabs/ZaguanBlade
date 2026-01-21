'use client';
import React, { useState, useEffect } from 'react';
import { Save, X, FileText } from 'lucide-react';
import { save } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { MarkdownRenderer } from './MarkdownRenderer';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { vscDarkPlus } from 'react-syntax-highlighter/dist/esm/styles/prism';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

interface DocumentViewerProps {
  documentId: string;
  title: string;
  content: string;
  isEphemeral: boolean;
  suggestedName?: string;
  onClose: () => void;
  onSave?: (path: string) => void;

}

export const DocumentViewer: React.FC<DocumentViewerProps> = ({
  documentId,
  title,
  content,
  isEphemeral,
  suggestedName,
  onClose,
  onSave,

}) => {
  const [isSaving, setIsSaving] = useState(false);

  const handleSave = async () => {
    if (!isEphemeral) return;



    // Otherwise, use the save dialog for generic ephemeral documents
    setIsSaving(true);
    try {
      const filePath = await save({
        defaultPath: suggestedName || 'document.md',
        filters: [{
          name: 'Markdown',
          extensions: ['md']
        }]
      });

      if (filePath) {
        await invoke('save_ephemeral_document', {
          id: documentId,
          path: filePath
        });

        if (onSave) {
          onSave(filePath);
        }
      }
    } catch (error) {
      console.error('Failed to save document:', error);
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
    <div className="flex flex-col h-full bg-zinc-950">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-zinc-800 bg-zinc-900/50">
        <div className="flex items-center gap-2">
          <FileText className="w-4 h-4 text-emerald-500" />
          <span className="text-sm font-medium text-zinc-200">{title}</span>
          {isEphemeral && (
            <span className="text-xs px-2 py-0.5 rounded bg-yellow-500/10 text-yellow-500 border border-yellow-500/20">
              Unsaved
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          {isEphemeral && (
            <button
              onClick={handleSave}
              disabled={isSaving}
              className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded bg-emerald-500/10 text-emerald-400 hover:bg-emerald-500/20 border border-emerald-500/20 transition-colors disabled:opacity-50"
            >
              <Save className="w-3.5 h-3.5" />
              {isSaving ? 'Saving...' : 'Save'}
            </button>
          )}
          <button
            onClick={handleClose}
            className="p-1.5 rounded hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto">
        <div className="max-w-4xl mx-auto px-6 py-8">
          <div className="prose prose-invert prose-lg max-w-none
            prose-headings:text-zinc-100 prose-headings:font-semibold prose-headings:tracking-tight
            prose-h1:text-3xl prose-h1:mb-4 prose-h1:mt-0
            prose-h2:text-2xl prose-h2:mb-3 prose-h2:mt-8
            prose-h3:text-xl prose-h3:mb-2 prose-h3:mt-6
            prose-p:text-zinc-300 prose-p:leading-relaxed prose-p:mb-4
            prose-strong:text-zinc-100 prose-strong:font-semibold
            prose-a:text-emerald-400 prose-a:no-underline hover:prose-a:underline
            prose-code:text-emerald-400 prose-code:bg-zinc-900 prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded prose-code:text-sm prose-code:before:content-[''] prose-code:after:content-['']
            prose-pre:bg-zinc-900 prose-pre:border prose-pre:border-zinc-800 prose-pre:rounded-lg prose-pre:shadow-xl
            prose-ul:text-zinc-300 prose-ul:my-4
            prose-ol:text-zinc-300 prose-ol:my-4
            prose-li:text-zinc-300 prose-li:leading-relaxed prose-li:my-1
            prose-blockquote:border-l-emerald-500 prose-blockquote:text-zinc-400 prose-blockquote:italic
            prose-hr:border-zinc-800 prose-hr:my-8
            prose-table:border-collapse prose-table:my-6
            prose-thead:border-b-2 prose-thead:border-zinc-700
            prose-th:text-zinc-200 prose-th:font-semibold prose-th:text-left prose-th:px-4 prose-th:py-3 prose-th:bg-zinc-900/50
            prose-td:text-zinc-300 prose-td:px-4 prose-td:py-3 prose-td:border-t prose-td:border-zinc-800
            prose-tr:transition-colors hover:prose-tr:bg-zinc-900/30">
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
      </div>
    </div>
  );
};
