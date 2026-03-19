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
    <div className="mx-3 mb-2 flex items-center justify-between gap-3 px-1 py-1.5 text-xs">
      <div className="flex items-baseline gap-2 text-sm font-medium text-[var(--fg-primary)]">
        <span>
          {fileCount} file{fileCount !== 1 ? 's' : ''}
        </span>
        <span style={{ color: 'var(--accent-green)' }}>+{totalAdded}</span>
        <span style={{ color: 'var(--accent-error)' }}>-{totalRemoved}</span>
      </div>

      <div className="flex items-center gap-3 text-[11px] font-medium">
        <button
          onClick={onAcceptAll}
          disabled={disabled}
          className="flex items-center gap-1 px-1 py-0.5 transition-colors disabled:cursor-not-allowed disabled:opacity-50 hover:opacity-80"
          style={{ color: 'var(--accent-green)' }}
          title={t('diff.acceptAll')}
        >
          <Check className="w-3 h-3" />
          {t('diff.acceptAll')}
        </button>
        <button
          onClick={onRejectAll}
          disabled={disabled}
          className="flex items-center gap-1 px-1 py-0.5 transition-colors disabled:cursor-not-allowed disabled:opacity-50 hover:opacity-80"
          style={{ color: 'var(--accent-error)' }}
          title={t('diff.rejectAll')}
        >
          <X className="w-3 h-3" />
          {t('diff.rejectAll')}
        </button>
      </div>
    </div>
  );
};

export default GlobalChangeActions;
