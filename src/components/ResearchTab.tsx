import React from 'react';
import { Search, Loader2, X } from 'lucide-react';
import ReactMarkdown from 'react-markdown';

export interface ResearchProgress {
    message: string;
    stage: string;
    percent: number;
    isActive: boolean;
}

interface ResearchTabProps {
    content: string | null;
    progress: ResearchProgress | null;
    onClose: () => void;
}

export const ResearchTab: React.FC<ResearchTabProps> = ({ content, progress, onClose }) => {
    // Show progress UI when research is active
    if (progress?.isActive) {
        return (
            <div className="flex-1 flex flex-col bg-[var(--bg-app)]">
                <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--border-subtle)]">
                    <div className="flex items-center gap-2">
                        <Search className="w-4 h-4 text-[var(--accent-primary)]" />
                        <span className="text-sm font-medium text-[var(--fg-primary)]">Research</span>
                    </div>
                </div>
                <div className="flex-1 flex items-center justify-center">
                    <div className="flex flex-col items-center gap-4 max-w-md px-6">
                        <Loader2 className="w-8 h-8 text-[var(--accent-primary)] animate-spin" />
                        <div className="text-center">
                            <p className="text-sm text-[var(--fg-primary)] mb-2">
                                {progress.message || 'Researching...'}
                            </p>
                            <div className="w-64 h-2 bg-[var(--bg-surface)] rounded-full overflow-hidden">
                                <div 
                                    className="h-full bg-[var(--accent-primary)] transition-all duration-300"
                                    style={{ width: `${progress.percent}%` }}
                                />
                            </div>
                            <p className="text-xs text-[var(--fg-tertiary)] mt-1">
                                {progress.percent}%
                            </p>
                        </div>
                    </div>
                </div>
            </div>
        );
    }

    // Show empty state if no content
    if (!content) {
        return (
            <div className="flex-1 flex items-center justify-center bg-[var(--bg-app)]">
                <div className="flex flex-col items-center gap-4 text-[var(--fg-tertiary)] select-none">
                    <Search className="w-12 h-12 opacity-20" />
                    <div className="text-center">
                        <h3 className="text-sm font-medium text-[var(--fg-secondary)]">
                            No Research Results
                        </h3>
                        <p className="text-xs opacity-70 mt-1">
                            Use @research &lt;query&gt; to start a deep web search
                        </p>
                    </div>
                </div>
            </div>
        );
    }

    // Show research results
    return (
        <div className="flex-1 flex flex-col bg-[var(--bg-app)]">
            <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--border-subtle)]">
                <div className="flex items-center gap-2">
                    <Search className="w-4 h-4 text-[var(--accent-primary)]" />
                    <span className="text-sm font-medium text-[var(--fg-primary)]">Research Results</span>
                </div>
                <button
                    onClick={onClose}
                    className="p-1 rounded hover:bg-[var(--bg-surface-hover)] text-[var(--fg-tertiary)] hover:text-[var(--fg-secondary)]"
                >
                    <X className="w-4 h-4" />
                </button>
            </div>
            <div className="flex-1 overflow-y-auto">
                <div className="max-w-4xl mx-auto py-4 px-6">
                    <div className="prose prose-sm prose-invert max-w-none
                        prose-headings:text-[var(--fg-primary)]
                        prose-p:text-[var(--fg-secondary)]
                        prose-a:text-[var(--accent-primary)]
                        prose-strong:text-[var(--fg-primary)]
                        prose-code:text-[var(--fg-primary)]
                        prose-code:bg-[var(--bg-surface)]
                        prose-code:px-1
                        prose-code:rounded
                        prose-pre:bg-[var(--bg-surface)]
                        prose-pre:border
                        prose-pre:border-[var(--border-subtle)]
                        prose-li:text-[var(--fg-secondary)]
                        prose-blockquote:border-[var(--accent-primary)]
                        prose-blockquote:text-[var(--fg-tertiary)]
                    ">
                        <ReactMarkdown>{content}</ReactMarkdown>
                    </div>
                </div>
            </div>
        </div>
    );
};
