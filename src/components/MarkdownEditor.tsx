import React, { forwardRef, useImperativeHandle, useState } from 'react';
import { useTranslation } from 'react-i18next';
import CodeEditor, { type CodeEditorHandle } from './CodeEditor';
import { MarkdownRenderer } from './MarkdownRenderer';
import { Eye, Edit3 } from 'lucide-react';
import { useUncommittedChanges } from '../hooks/useUncommittedChanges';
import { FileChangeBar } from './editor/FileChangeBar';
import { ScrollArea } from './ui/ScrollArea';

interface MarkdownEditorProps {
    content: string;
    onDocumentChange?: () => void;
    onSave?: (val: string) => void;
    filename?: string;
    externalContentVersion?: number;
}

export const MarkdownEditor = forwardRef<CodeEditorHandle, MarkdownEditorProps>(({
    content,
    onDocumentChange,
    onSave,
    filename,
    externalContentVersion,
}, ref) => {
    const { t } = useTranslation();
    const [mode, setMode] = useState<'edit' | 'view'>('edit');
    const [previewContent, setPreviewContent] = useState(content);
    const previewContentRef = React.useRef(content);
    const editorRef = React.useRef<CodeEditorHandle>(null);

    const { getChangeForFile, acceptFile, rejectFile, refresh } = useUncommittedChanges();
    const change = filename ? getChangeForFile(filename) : undefined;

    useImperativeHandle(ref, () => ({
        getView: () => editorRef.current?.getView() ?? null,
        getContent: () => editorRef.current?.getContent() ?? previewContentRef.current,
        replaceDocument: (input) => {
            editorRef.current?.replaceDocument(input);
            previewContentRef.current = input.content;
            setPreviewContent(input.content);
        },
        setCursor: (line: number, col: number) => {
            editorRef.current?.setCursor(line, col);
        },
    }), []);

    React.useEffect(() => {
        if (mode === 'edit') {
            previewContentRef.current = content;
            setPreviewContent(content);
        }
    }, [content, mode]);

    const switchMode = React.useCallback(() => {
        if (mode === 'edit') {
            const nextPreviewContent = editorRef.current?.getContent() ?? previewContentRef.current;
            previewContentRef.current = nextPreviewContent;
            setPreviewContent(nextPreviewContent);
            setMode('view');
        } else {
            setMode('edit');
        }
    }, [mode]);

    const handleAccept = async () => {
        if (filename) await acceptFile(filename);
    };

    const handleReject = async () => {
        if (filename) {
            await rejectFile(filename);
            setTimeout(() => {
                refresh();
            }, 100);
        }
    };

    // Keyboard shortcut for mode toggle
    React.useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // Ctrl+E to toggle between Edit and View mode
            if (e.ctrlKey && e.key === 'e' && !e.shiftKey) {
                e.preventDefault();
                switchMode();
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [switchMode]);

    return (
        <div className="h-full flex flex-col bg-(--editor-bg)">
            {/* Toggle Bar */}
            <div className="h-10 border-b border-(--markdown-border) flex items-center justify-between px-4" style={{ backgroundColor: 'var(--markdown-block-header-bg)' }}>
                <div className="flex items-center gap-2">
                    <span className="text-xs text-(--markdown-marker) font-mono">
                        {filename?.split('/').pop() || t('editor.untitled')}
                    </span>
                </div>

                <div className="flex items-center gap-1 rounded-[calc(var(--panel-radius)*0.45)] p-0.5" style={{ backgroundColor: 'color-mix(in srgb, var(--markdown-inline-code-bg) 80%, transparent)' }}>
                    <button
                        onClick={switchMode}
                        className={`flex items-center gap-2 px-3 py-1 rounded-[calc(var(--panel-radius)*0.35)] text-xs font-medium transition-all ${mode === 'edit'
                                ? 'shadow-(--shadow-sm) border text-(--markdown-heading) border-(--markdown-border) bg-(--markdown-inline-code-bg)'
                                : 'shadow-(--shadow-sm) border border-(--accent-ai) text-(--markdown-link)'
                            }`}
                        style={mode === 'view' ? { backgroundColor: 'color-mix(in srgb, var(--accent-ai) 18%, transparent)' } : undefined}
                        title={t('editor.toggleEditView')}
                    >
                        {mode === 'edit' ? (
                            <>
                                <Edit3 className="w-3.5 h-3.5" />
                                <span>{t('editor.editing')}</span>
                            </>
                        ) : (
                            <>
                                <Eye className="w-3.5 h-3.5" />
                                <span>{t('editor.viewing')}</span>
                            </>
                        )}
                    </button>
                    {/* Helper hint */}
                    <span className="text-[10px] text-(--markdown-marker) px-1 font-mono hidden sm:inline-block">
                        Ctrl+E
                    </span>
                </div>
            </div>

            {/* Content Area */}
            <div className="flex-1 overflow-hidden relative">
                {mode === 'edit' ? (
                    <>
                        <CodeEditor
                            ref={editorRef}
                            content={content}
                            onDocumentChange={onDocumentChange}
                            onSave={onSave}
                            filename={filename}
                            externalContentVersion={externalContentVersion}
                            unifiedDiff={change?.unified_diff}
                        />
                        {change && (
                            <FileChangeBar
                                change={change}
                                onAccept={handleAccept}
                                onReject={handleReject}
                            />
                        )}
                    </>
                ) : (
                    <ScrollArea className="h-full px-8 py-6 pb-[35vh] bg-(--editor-bg)" style={{ scrollPaddingBottom: '35vh' }}>
                        <div className="max-w-4xl mx-auto">
                            <MarkdownRenderer content={previewContent} />
                        </div>
                        <div aria-hidden="true" className="h-[35vh]" />
                    </ScrollArea>
                )}
            </div>
        </div>
    );
});
