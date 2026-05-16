import React from 'react';
import { cn } from '../../lib/utils';

interface ThemedDropdownSurfaceProps extends React.HTMLAttributes<HTMLDivElement> {}

export const ThemedDropdownSurface: React.FC<ThemedDropdownSurfaceProps> = ({ className, ...props }) => (
    <div
        className={cn(
            'z-50 overflow-hidden rounded-[calc(var(--panel-radius)+6px)] border border-(--border-default) bg-[color-mix(in_srgb,var(--bg-panel)_92%,var(--bg-surface))] p-2 shadow-(--panel-shadow)',
            className,
        )}
        {...props}
    />
);

interface ThemedDropdownScrollAreaProps extends React.HTMLAttributes<HTMLDivElement> {}

export const ThemedDropdownScrollArea = React.forwardRef<HTMLDivElement, ThemedDropdownScrollAreaProps>(({ className, ...props }, ref) => (
    <div ref={ref} className={cn('space-y-1 overflow-y-auto pr-1', className)} {...props} />
));

ThemedDropdownScrollArea.displayName = 'ThemedDropdownScrollArea';

interface ThemedDropdownEmptyStateProps extends React.HTMLAttributes<HTMLDivElement> {}

export const ThemedDropdownEmptyState: React.FC<ThemedDropdownEmptyStateProps> = ({ className, ...props }) => (
    <div className={cn('px-3 py-2 text-center italic text-(--fg-tertiary)', className)} {...props} />
);

interface ThemedDropdownSectionLabelProps extends React.HTMLAttributes<HTMLDivElement> {}

export const ThemedDropdownSectionLabel: React.FC<ThemedDropdownSectionLabelProps> = ({ className, ...props }) => (
    <div
        className={cn(
            'border-t border-(--border-subtle) px-2 pt-2 text-[10px] font-semibold uppercase tracking-[0.16em] text-(--fg-tertiary)',
            className,
        )}
        {...props}
    />
);

export function themedDropdownItemClassName(selected: boolean, className?: string) {
    return cn(
        'group w-full rounded-[calc(var(--panel-radius)+2px)] border text-left transition-[border-color,background-color,box-shadow,transform] duration-200 focus:outline-none',
        selected
            ? 'border-(--accent-ai) bg-[color-mix(in_srgb,var(--accent-ai)_14%,var(--bg-surface))] ring-1 ring-(--accent-ai)/30 shadow-(--shadow-sm)'
            : 'border-transparent bg-(--bg-surface) hover:border-(--border-default) hover:bg-(--bg-surface-hover) focus:border-(--accent-ai) focus:bg-(--bg-surface-hover)',
        className,
    );
}
