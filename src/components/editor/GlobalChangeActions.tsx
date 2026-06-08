import React from 'react';
import { useTranslation } from 'react-i18next';
import { Check, X } from 'lucide-react';
import type { UncommittedChange } from '../../types/uncommitted';

interface GlobalChangeActionsProps {
  changes: UncommittedChange[];
  onAcceptAll: () => void;
  onRejectAll: () => void;
  disabled?: boolean;
}

export const GlobalChangeActions: React.FC<GlobalChangeActionsProps> = ({
  changes,
  onAcceptAll,
  onRejectAll,
  disabled = false,
}) => {
  const { t } = useTranslation();
  if (changes.length === 0) {
    return null;
  }

  const totalAdded = changes.reduce((sum, c) => sum + c.added_lines, 0);
  const totalRemoved = changes.reduce((sum, c) => sum + c.removed_lines, 0);
  const fileCount = new Set(changes.map(c => c.file_path)).size;

  return (
    <div className="shrink-0 bg-(--bg-panel) px-2 pt-1.5">
      <div className="flex items-center justify-between gap-3 px-1 py-1.5 text-xs">
        <div className="flex min-w-0 items-baseline gap-2 text-xs font-medium text-(--fg-primary)">
          <span className="truncate">
            {fileCount} file{fileCount !== 1 ? 's' : ''}
          </span>
          <span className="text-(--accent-mention)">+{totalAdded}</span>
          <span className="text-(--state-danger)">-{totalRemoved}</span>
        </div>

        <div className="flex shrink-0 items-center gap-1.5 text-[11px] font-medium">
          <button
            type="button"
            onClick={onRejectAll}
            disabled={disabled}
            className="flex items-center gap-1 rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-panel)/60 px-2 py-1 text-(--fg-tertiary) transition-colors hover:border-[color-mix(in_srgb,var(--state-danger)_26%,transparent)] hover:bg-[color-mix(in_srgb,var(--state-danger)_8%,transparent)] hover:text-(--state-danger) disabled:cursor-not-allowed disabled:opacity-50"
            title={t('diff.rejectAll')}
            aria-label={t('diff.rejectAll')}
          >
            <X className="w-3 h-3" aria-hidden="true" />
            {t('diff.rejectAll')}
          </button>
          <button
            type="button"
            onClick={onAcceptAll}
            disabled={disabled}
            className="flex items-center gap-1 rounded-[calc(var(--panel-radius)*0.45)] border border-[color-mix(in_srgb,var(--accent-mention)_22%,transparent)] bg-[color-mix(in_srgb,var(--accent-mention)_10%,transparent)] px-2 py-1 text-(--accent-mention) transition-colors hover:border-[color-mix(in_srgb,var(--accent-mention)_34%,transparent)] hover:bg-[color-mix(in_srgb,var(--accent-mention)_14%,transparent)] hover:text-(--fg-bright) disabled:cursor-not-allowed disabled:opacity-50"
            title={t('diff.acceptAll')}
            aria-label={t('diff.acceptAll')}
          >
            <Check className="w-3 h-3" aria-hidden="true" />
            {t('diff.acceptAll')}
          </button>
        </div>
      </div>
    </div>
  );
};

export default GlobalChangeActions;
