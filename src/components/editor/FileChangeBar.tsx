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
    <div className="flex items-center justify-between px-3 py-1.5 bg-[var(--bg-tertiary)] border-b border-[var(--border-primary)]">
      <div className="flex items-center gap-3">
        <span className="text-xs font-medium text-[var(--fg-secondary)]">
          AI Changed
        </span>
        <span className="text-xs text-[var(--fg-primary)] font-mono">
          {fileName}
        </span>
        <div className="flex items-center gap-2 text-xs">
          <span className="flex items-center gap-0.5 text-green-500">
            <Plus className="w-3 h-3" />
            {change.added_lines}
          </span>
          <span className="flex items-center gap-0.5 text-red-500">
            <Minus className="w-3 h-3" />
            {change.removed_lines}
          </span>
        </div>
      </div>

      <div className="flex items-center gap-2">
        <button
          onClick={onAccept}
          disabled={disabled}
          className="flex items-center gap-1 px-2 py-1 text-xs font-medium text-green-400 bg-green-500/10 hover:bg-green-500/20 rounded transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          title="Accept changes (keep on disk)"
        >
          <Check className="w-3.5 h-3.5" />
          Accept
        </button>
        <button
          onClick={onReject}
          disabled={disabled}
          className="flex items-center gap-1 px-2 py-1 text-xs font-medium text-red-400 bg-red-500/10 hover:bg-red-500/20 rounded transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          title="Reject changes (revert to original)"
        >
          <X className="w-3.5 h-3.5" />
          Reject
        </button>
      </div>
    </div>
  );
};

export default FileChangeBar;
