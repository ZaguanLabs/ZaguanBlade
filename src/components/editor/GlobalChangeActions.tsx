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
      <div className="rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface)/55 px-2.5 py-2 shadow-(--shadow-sm)">
        <div className="mb-1.5 flex items-center gap-2 text-[9px] font-semibold uppercase tracking-[0.16em] text-(--accent-mention)">
          Review
          <span className="rounded-full border border-(--border-subtle) bg-(--bg-app) px-1.5 py-0.5 text-[9px] leading-none text-(--fg-tertiary)">
            {fileCount}
          </span>
        </div>
        <div className="flex items-center justify-between gap-3 text-xs">
          <div className="flex min-w-0 items-baseline gap-2 text-sm font-medium text-(--fg-primary)">
            <span className="truncate">
              {fileCount} file{fileCount !== 1 ? 's' : ''}
            </span>
            <span className="text-(--accent-mention)">+{totalAdded}</span>
            <span className="text-(--state-danger)">-{totalRemoved}</span>
          </div>

          <div className="flex shrink-0 items-center gap-3 text-[11px] font-medium">
            <button
              type="button"
              onClick={onAcceptAll}
              disabled={disabled}
              className="flex items-center gap-1 px-1 py-0.5 text-(--accent-mention) transition-colors disabled:cursor-not-allowed disabled:opacity-50 hover:text-(--fg-primary)"
              title={t('diff.acceptAll')}
              aria-label={t('diff.acceptAll')}
            >
              <Check className="w-3 h-3" />
              {t('diff.acceptAll')}
            </button>
            <button
              type="button"
              onClick={onRejectAll}
              disabled={disabled}
              className="flex items-center gap-1 px-1 py-0.5 text-(--state-danger) transition-colors disabled:cursor-not-allowed disabled:opacity-50 hover:text-(--fg-primary)"
              title={t('diff.rejectAll')}
              aria-label={t('diff.rejectAll')}
            >
              <X className="w-3 h-3" />
              {t('diff.rejectAll')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

export default GlobalChangeActions;
