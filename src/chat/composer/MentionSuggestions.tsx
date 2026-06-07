import React from 'react';
import { BookOpen, FileText, Folder, Globe } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { WorkspacePathMatch } from './usePathSuggestions';

export type ComposerSuggestion =
    | { kind: 'command'; key: string; name: 'web' | 'research' }
    | { kind: 'path'; key: string; entry: WorkspacePathMatch };

export const MentionSuggestions: React.FC<{
    suggestions: ComposerSuggestion[];
    selectedIndex: number;
    onSelect: (suggestion: ComposerSuggestion) => void;
}> = ({ suggestions, selectedIndex, onSelect }) => {
    const { t } = useTranslation();

    if (suggestions.length === 0) {
        return null;
    }

    return (
        <div
            role="listbox"
            aria-label={t('chat.mentionSuggestions')}
            className="absolute bottom-full left-0 right-0 z-80 mb-1.5 overflow-hidden rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface) shadow-(--shadow-lg)"
        >
            {suggestions.map((suggestion, index) => {
                const isSelected = index === selectedIndex;
                if (suggestion.kind === 'command') {
                    const Icon = suggestion.name === 'web' ? Globe : BookOpen;
                    return (
                        <button
                            key={suggestion.key}
                            type="button"
                            role="option"
                            aria-selected={isSelected}
                            onClick={() => onSelect(suggestion)}
                            className={`flex w-full items-center gap-2 px-2 py-1.5 text-left ${isSelected ? 'bg-(--accent-ai)/15 text-(--fg-primary)' : 'text-(--fg-secondary) hover:bg-(--bg-surface-hover)'}`}
                        >
                            <Icon className="h-3.5 w-3.5 shrink-0 text-(--accent-ai)" aria-hidden="true" />
                            <span className="min-w-0 flex-1 truncate text-[11px]">@{suggestion.name}</span>
                            <span className="text-[9px] text-(--fg-tertiary)">command</span>
                        </button>
                    );
                }

                const Icon = suggestion.entry.is_dir ? Folder : FileText;
                return (
                    <button
                        key={suggestion.key}
                        type="button"
                        role="option"
                        aria-selected={isSelected}
                        onClick={() => onSelect(suggestion)}
                        className={`flex w-full items-center gap-2 px-2 py-1.5 text-left ${isSelected ? 'bg-(--accent-ai)/15 text-(--fg-primary)' : 'text-(--fg-secondary) hover:bg-(--bg-surface-hover)'}`}
                    >
                        <Icon className="h-3.5 w-3.5 shrink-0 text-(--accent-ai)" aria-hidden="true" />
                        <span className="min-w-0 flex-1 truncate text-[11px]">@{suggestion.entry.path}</span>
                        <span className="text-[9px] text-(--fg-tertiary)">{suggestion.entry.is_dir ? 'folder' : 'file'}</span>
                    </button>
                );
            })}
        </div>
    );
};
