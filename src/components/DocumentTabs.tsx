'use client';
import React from 'react';
import { X, FileText } from 'lucide-react';
import { getFileIcon } from '../lib/fileIcons';

interface Tab {
  id: string;
  title: string;
  isEphemeral?: boolean;
  isDirty?: boolean;
  hasVirtualChanges?: boolean;
}

interface DocumentTabsProps {
  tabs: Tab[];
  activeTabId: string | null;
  onTabClick: (id: string) => void;
  onTabClose: (id: string) => void;
}

export const DocumentTabs: React.FC<DocumentTabsProps> = ({
  tabs,
  activeTabId,
  onTabClick,
  onTabClose,
}) => {
  if (tabs.length === 0) return null;

  return (
    <div className="flex items-center bg-[#252526] border-b border-[#3c3c3c] overflow-x-auto">
      {tabs.map((tab) => {
        const isActive = activeTabId === tab.id;
        const { icon, color } = getFileIcon(tab.title, isActive);
        return (
          <div
            key={tab.id}
            onClick={() => onTabClick(tab.id)}
            className={`
              group flex items-center gap-2 px-3 py-2 min-w-[120px] max-w-[200px] cursor-pointer
              border-r border-[#3c3c3c] transition-colors relative
              ${isActive
                ? 'bg-[#1e1e1e] text-white border-t-2 border-t-emerald-500' 
                : 'bg-[#2d2d2d] text-zinc-400 hover:bg-[#323233]'
              }
            `}
          >
            {tab.isEphemeral ? (
              <FileText className={`w-3.5 h-3.5 shrink-0 ${isActive ? 'text-yellow-400' : 'text-yellow-500'}`} />
            ) : (
              <span className={color}>{icon}</span>
            )}
            <span className={`text-xs truncate flex-1 ${isActive ? 'font-semibold' : ''}`}>
              {tab.title}
            </span>
            {tab.hasVirtualChanges && (
              <span 
                className="w-1.5 h-1.5 rounded-full bg-orange-400 shrink-0 animate-pulse" 
                title="Virtual changes (not saved to disk)"
              />
            )}
            {tab.isDirty && !tab.hasVirtualChanges && (
              <span className="w-1.5 h-1.5 rounded-full bg-white shrink-0 group-hover:hidden" />
            )}
            <button
              onClick={(e) => {
                e.stopPropagation();
                onTabClose(tab.id);
              }}
              className="opacity-0 group-hover:opacity-100 hover:bg-zinc-700 rounded p-0.5 transition-all"
            >
              <X className="w-3 h-3" />
            </button>
          </div>
        );
      })}
    </div>
  );
};
