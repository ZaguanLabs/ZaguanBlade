'use client';
import React, { useState, useCallback } from 'react';
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
  onReorder?: (fromIndex: number, toIndex: number) => void;
}

export const DocumentTabs: React.FC<DocumentTabsProps> = ({
  tabs,
  activeTabId,
  onTabClick,
  onTabClose,
  onReorder,
}) => {
  const scrollRef = React.useRef<HTMLDivElement>(null);
  const activeTabRef = React.useRef<HTMLDivElement>(null);
  const [draggedIndex, setDraggedIndex] = useState<number | null>(null);
  const [dropTargetIndex, setDropTargetIndex] = useState<number | null>(null);

  const handleDragStart = useCallback((e: React.DragEvent, index: number) => {
    setDraggedIndex(index);
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', String(index));
    // Make drag image slightly transparent
    if (e.currentTarget instanceof HTMLElement) {
      e.currentTarget.style.opacity = '0.5';
    }
  }, []);

  const handleDragEnd = useCallback((e: React.DragEvent) => {
    setDraggedIndex(null);
    setDropTargetIndex(null);
    if (e.currentTarget instanceof HTMLElement) {
      e.currentTarget.style.opacity = '1';
    }
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent, index: number) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    if (draggedIndex !== null && draggedIndex !== index) {
      setDropTargetIndex(index);
    }
  }, [draggedIndex]);

  const handleDragLeave = useCallback(() => {
    setDropTargetIndex(null);
  }, []);

  const handleDrop = useCallback((e: React.DragEvent, toIndex: number) => {
    e.preventDefault();
    const fromIndex = draggedIndex;
    setDraggedIndex(null);
    setDropTargetIndex(null);
    
    if (fromIndex !== null && fromIndex !== toIndex && onReorder) {
      onReorder(fromIndex, toIndex);
    }
  }, [draggedIndex, onReorder]);

  React.useEffect(() => {
    if (activeTabId && activeTabRef.current) {
      activeTabRef.current.scrollIntoView({
        behavior: 'smooth',
        block: 'nearest',
        inline: 'nearest',
      });
    }
  }, [activeTabId]);

  if (tabs.length === 0) return null;

  return (
    <div
      ref={scrollRef}
      className="flex items-center bg-[#252526] border-b border-[#3c3c3c] overflow-x-auto tabs-scrollbar"
    >
      {tabs.map((tab, index) => {
        const isActive = activeTabId === tab.id;
        const { icon, color } = getFileIcon(tab.title, isActive);
        const isDragging = draggedIndex === index;
        const isDropTarget = dropTargetIndex === index;
        return (
          <div
            key={tab.id}
            ref={isActive ? activeTabRef : null}
            draggable
            onDragStart={(e) => handleDragStart(e, index)}
            onDragEnd={handleDragEnd}
            onDragOver={(e) => handleDragOver(e, index)}
            onDragLeave={handleDragLeave}
            onDrop={(e) => handleDrop(e, index)}
            onClick={() => onTabClick(tab.id)}
            className={`
              group flex items-center gap-2 px-3 py-2 cursor-pointer
              border-r border-[#3c3c3c] transition-colors relative whitespace-nowrap
              ${isActive
                ? 'bg-[#1e1e1e] text-white border-t-2 border-t-emerald-500'
                : 'bg-[#2d2d2d] text-zinc-400 hover:bg-[#323233]'
              }
              ${isDragging ? 'opacity-50' : ''}
              ${isDropTarget ? 'border-l-2 border-l-emerald-400' : ''}
            `}
          >
            {tab.isEphemeral ? (
              <FileText className={`w-3.5 h-3.5 shrink-0 ${isActive ? 'text-yellow-400' : 'text-yellow-500'}`} />
            ) : (
              <span className={color}>{icon}</span>
            )}
            <span className={`text-xs ${isActive ? 'font-semibold' : ''}`}>
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
              className={`${isActive ? 'opacity-100' : 'opacity-0'} group-hover:opacity-100 hover:bg-zinc-700 rounded p-0.5 transition-all`}
            >
              <X className="w-3 h-3" />
            </button>
          </div>
        );
      })}
    </div>
  );
};
