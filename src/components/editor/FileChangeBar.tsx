import React from 'react';
import { useTranslation } from 'react-i18next';
import { Check, X, Plus, Minus } from 'lucide-react';
import type { UncommittedChange } from '../../types/uncommitted';

interface FileChangeBarProps {
  change: UncommittedChange;
  onAccept: () => void;
  onReject: () => void;
  disabled?: boolean;
}

export const FileChangeBar: React.FC<FileChangeBarProps> = ({
  change,
  onAccept,
  onReject,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const fileName = change.file_path.split('/').pop() || change.file_path;

  return (
    <div
      className="absolute top-2 right-4 z-20 flex items-center gap-2 px-3 py-1.5 select-none"
      style={{
        backgroundColor: 'var(--bg-surface)',
        border: '1px solid var(--border-default)',
        borderRadius: '9999px',
        boxShadow: 'var(--panel-shadow)',
      }}
    >
      {/* File name */}
      <span className="text-[11px] font-medium font-mono" style={{ color: 'var(--fg-secondary)' }}>
        {fileName}
      </span>

      {/* Diff stats */}
      <div className="flex items-center gap-1.5 text-[11px]">
        {change.added_lines > 0 && (
          <span className="flex items-center gap-0.5" style={{ color: 'var(--accent-mention)' }}>
            <Plus className="w-3 h-3" />
            {change.added_lines}
          </span>
        )}
        {change.removed_lines > 0 && (
          <span className="flex items-center gap-0.5" style={{ color: 'var(--state-danger)' }}>
            <Minus className="w-3 h-3" />
            {change.removed_lines}
          </span>
        )}
      </div>

      {/* Divider */}
      <div className="w-px h-3.5" style={{ backgroundColor: 'var(--border-default)' }} />

      {/* Accept */}
      <button
        type="button"
        onClick={onAccept}
        disabled={disabled}
        className="flex items-center gap-1 text-[11px] font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        style={{ color: 'var(--accent-mention)' }}
        title={t('diff.acceptChangesTitle')}
        aria-label={t('diff.acceptChangesTitle')}
      >
        <Check className="w-3.5 h-3.5" />
        {t('diff.accept')}
      </button>

      {/* Reject */}
      <button
        type="button"
        onClick={onReject}
        disabled={disabled}
        className="flex items-center gap-1 text-[11px] font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        style={{ color: 'var(--state-danger)' }}
        title={t('diff.rejectChangesTitle')}
        aria-label={t('diff.rejectChangesTitle')}
      >
        <X className="w-3.5 h-3.5" />
        {t('diff.reject')}
      </button>
    </div>
  );
};

export default FileChangeBar;
