import React from 'react';
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
        onClick={onAccept}
        disabled={disabled}
        className="flex items-center gap-1 text-[11px] font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        style={{ color: 'var(--accent-mention)' }}
        title="Accept changes (keep on disk)"
      >
        <Check className="w-3.5 h-3.5" />
        Accept
      </button>

      {/* Reject */}
      <button
        onClick={onReject}
        disabled={disabled}
        className="flex items-center gap-1 text-[11px] font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        style={{ color: 'var(--state-danger)' }}
        title="Reject changes (revert to original)"
      >
        <X className="w-3.5 h-3.5" />
        Reject
      </button>
    </div>
  );
};

export default FileChangeBar;
