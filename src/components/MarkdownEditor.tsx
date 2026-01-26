import React, { useState } from 'react';
import CodeEditor, { type CodeEditorHandle } from './CodeEditor';
import { MarkdownRenderer } from './MarkdownRenderer';
import { Eye, Edit3 } from 'lucide-react';

interface MarkdownEditorProps {
    content: string;
    onChange: (val: string) => void;
    onSave?: (val: string) => void;
    filename?: string;
}

export const MarkdownEditor: React.FC<MarkdownEditorProps> = ({
    content,
    onChange,
    onSave,
    filename
}) => {
    const [mode, setMode] = useState<'edit' | 'view'>('edit');
    const editorRef = React.useRef<CodeEditorHandle>(null);

    // Keyboard shortcut for mode toggle
    React.useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // Ctrl+E to toggle between Edit and View mode
            if (e.ctrlKey && e.key === 'e' && !e.shiftKey) {
                e.preventDefault();
                setMode(prev => prev === 'edit' ? 'view' : 'edit');
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, []);

    return (
        <div className="h-full flex flex-col bg-[#1e1e1e]">
            {/* Toggle Bar */}
            <div className="h-10 bg-zinc-900/50 border-b border-zinc-800 flex items-center justify-between px-4">
                <div className="flex items-center gap-2">
                    <span className="text-xs text-zinc-500 font-mono">
                        {filename?.split('/').pop() || 'Untitled.md'}
                    </span>
                </div>

                <div className="flex items-center gap-1 bg-zinc-800/50 rounded-md p-0.5">
                    <button
                        onClick={() => setMode(mode === 'edit' ? 'view' : 'edit')}
                        className={`flex items-center gap-2 px-3 py-1 rounded text-xs font-medium transition-all ${mode === 'edit'
                                ? 'bg-zinc-700 text-zinc-100 shadow-sm border border-zinc-600'
                                : 'bg-emerald-900/40 text-emerald-200 shadow-sm border border-emerald-800/50'
                            }`}
                        title="Toggle Edit/View Mode (Ctrl+E)"
                    >
                        {mode === 'edit' ? (
                            <>
                                <Edit3 className="w-3.5 h-3.5" />
                                <span>Editing</span>
                            </>
                        ) : (
                            <>
                                <Eye className="w-3.5 h-3.5" />
                                <span>Viewing</span>
                            </>
                        )}
                    </button>
                    {/* Helper hint */}
                    <span className="text-[10px] text-zinc-500 px-1 font-mono hidden sm:inline-block">
                        Ctrl+E
                    </span>
                </div>
            </div>

            {/* Content Area */}
            <div className="flex-1 overflow-hidden">
                {mode === 'edit' ? (
                    <CodeEditor
                        ref={editorRef}
                        content={content}
                        onChange={onChange}
                        onSave={onSave}
                        filename={filename}
                    />
                ) : (
                    <div className="h-full overflow-y-auto px-8 py-6 bg-[#1e1e1e]">
                        <div className="max-w-4xl mx-auto">
                            <MarkdownRenderer content={content} />
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
};
