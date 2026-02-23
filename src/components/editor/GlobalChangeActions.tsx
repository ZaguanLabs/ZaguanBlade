import React from 'react';
import { Check, X, FileCode } from 'lucide-react';
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
  if (changes.length === 0) {
    return null;
  }

  const totalAdded = changes.reduce((sum, c) => sum + c.added_lines, 0);
  const totalRemoved = changes.reduce((sum, c) => sum + c.removed_lines, 0);
  const fileCount = new Set(changes.map(c => c.file_path)).size;

  return (
    <div className="flex items-center justify-end gap-2 px-3 py-1.5 text-xs">
      <div className="flex items-center gap-1.5 text-[var(--fg-secondary)]">
        <FileCode className="w-3 h-3" />
        <span>{fileCount} file{fileCount !== 1 ? 's' : ''}</span>
        <span style={{ color: 'var(--accent-secondary)' }}>+{totalAdded}</span>
        <span style={{ color: 'var(--accent-error)' }}>-{totalRemoved}</span>
      </div>

      <div className="w-px h-4 bg-[var(--border-primary)]" />

      <div className="flex items-center gap-1.5">
        <button
          onClick={onAcceptAll}
          disabled={disabled}
          className="flex items-center gap-1 font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          style={{ color: 'var(--accent-secondary)' }}
          title="Accept all AI changes"
        >
          <Check className="w-3 h-3" />
          Accept All
        </button>
        <button
          onClick={onRejectAll}
          disabled={disabled}
          className="flex items-center gap-1 font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          style={{ color: 'var(--accent-error)' }}
          title="Reject all AI changes (revert to original)"
        >
          <X className="w-3 h-3" />
          Reject All
        </button>
      </div>
    </div>
  );
};

export default GlobalChangeActions;
