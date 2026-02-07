import React, { useEffect, useRef, useState, useCallback } from 'react';
import { X, Check } from 'lucide-react';

interface RegionSelectorProps {
    isOpen: boolean;
    dataUrl: string;
    imageWidth: number;
    imageHeight: number;
    onCancel: () => void;
    onConfirm: (region: { x: number; y: number; width: number; height: number }) => void;
}

type Point = { x: number; y: number };

export const RegionSelector: React.FC<RegionSelectorProps> = ({
    isOpen,
    dataUrl,
    imageWidth,
    imageHeight,
    onCancel,
    onConfirm,
}) => {
    const wrapperRef = useRef<HTMLDivElement>(null);
    const [dragStart, setDragStart] = useState<Point | null>(null);
    const [dragEnd, setDragEnd] = useState<Point | null>(null);
    const [isDragging, setIsDragging] = useState(false);
    const [imgSize, setImgSize] = useState<{ w: number; h: number } | null>(null);

    useEffect(() => {
        if (!isOpen) {
            setDragStart(null);
            setDragEnd(null);
            setIsDragging(false);
            setImgSize(null);
        }
    }, [isOpen]);

    const handleImageLoad = useCallback((e: React.SyntheticEvent<HTMLImageElement>) => {
        const img = e.currentTarget;
        setImgSize({ w: img.clientWidth, h: img.clientHeight });
    }, []);

    if (!isOpen) return null;

    const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

    const getRelativePoint = (event: React.MouseEvent): Point | null => {
        const rect = wrapperRef.current?.getBoundingClientRect();
        if (!rect) return null;
        return {
            x: clamp(event.clientX - rect.left, 0, rect.width),
            y: clamp(event.clientY - rect.top, 0, rect.height),
        };
    };

    const handleMouseDown = (event: React.MouseEvent) => {
        event.preventDefault();
        const point = getRelativePoint(event);
        if (!point) return;
        setDragStart(point);
        setDragEnd(point);
        setIsDragging(true);
    };

    const handleMouseMove = (event: React.MouseEvent) => {
        if (!isDragging) return;
        const point = getRelativePoint(event);
        if (!point) return;
        setDragEnd(point);
    };

    const handleMouseUp = () => {
        if (!isDragging) return;
        setIsDragging(false);
    };

    const selection = dragStart && dragEnd
        ? {
            left: Math.min(dragStart.x, dragEnd.x),
            top: Math.min(dragStart.y, dragEnd.y),
            width: Math.abs(dragEnd.x - dragStart.x),
            height: Math.abs(dragEnd.y - dragStart.y),
        }
        : null;

    const handleConfirm = () => {
        if (!selection || !imgSize) return;
        const scaleX = imageWidth / imgSize.w;
        const scaleY = imageHeight / imgSize.h;
        const x = Math.round(selection.left * scaleX);
        const y = Math.round(selection.top * scaleY);
        const width = Math.round(selection.width * scaleX);
        const height = Math.round(selection.height * scaleY);
        if (width <= 0 || height <= 0) return;
        onConfirm({ x, y, width, height });
    };

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80">
            <div className="relative w-full h-full max-w-6xl max-h-[90vh] mx-6 bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded-xl shadow-2xl overflow-hidden flex flex-col">
                <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)] shrink-0">
                    <div>
                        <div className="text-sm font-semibold text-[var(--fg-primary)]">Capture Region</div>
                        <div className="text-xs text-[var(--fg-tertiary)]">Drag to select an area</div>
                    </div>
                    <button
                        onClick={onCancel}
                        className="p-1 text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] rounded transition"
                    >
                        <X className="w-4 h-4" />
                    </button>
                </div>
                <div className="flex-1 flex items-center justify-center bg-black overflow-hidden min-h-0">
                    <div
                        ref={wrapperRef}
                        className="relative inline-block select-none"
                        style={{ cursor: isDragging ? 'crosshair' : 'crosshair' }}
                        onMouseDown={handleMouseDown}
                        onMouseMove={handleMouseMove}
                        onMouseUp={handleMouseUp}
                        onMouseLeave={handleMouseUp}
                    >
                        <img
                            src={dataUrl}
                            alt="Screenshot"
                            onLoad={handleImageLoad}
                            className="block max-w-full max-h-[calc(90vh-120px)] select-none pointer-events-none"
                            draggable={false}
                        />
                        {selection && selection.width > 1 && selection.height > 1 && (
                            <div
                                className="absolute border-2 border-[var(--accent-primary)] rounded-sm pointer-events-none"
                                style={{
                                    left: `${selection.left}px`,
                                    top: `${selection.top}px`,
                                    width: `${selection.width}px`,
                                    height: `${selection.height}px`,
                                    backgroundColor: 'rgba(59, 130, 246, 0.15)',
                                }}
                            />
                        )}
                    </div>
                </div>
                <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-[var(--border-subtle)] shrink-0">
                    <button
                        onClick={onCancel}
                        className="px-3 py-1.5 text-xs font-medium text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] rounded transition"
                    >
                        Cancel
                    </button>
                    <button
                        onClick={handleConfirm}
                        disabled={!selection || selection.width < 5 || selection.height < 5}
                        className="px-3 py-1.5 text-xs font-medium bg-[var(--accent-primary)] text-white rounded transition disabled:opacity-50 disabled:cursor-not-allowed inline-flex items-center gap-1.5"
                    >
                        <Check className="w-3.5 h-3.5" />
                        Capture
                    </button>
                </div>
            </div>
        </div>
    );
};
