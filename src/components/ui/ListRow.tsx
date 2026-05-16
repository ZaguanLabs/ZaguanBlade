import React from 'react';
import { cn } from '../../lib/utils';

type ListRowVariant = 'plain' | 'card';

interface ListRowProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
    variant?: ListRowVariant;
}

const listRowVariantClassName: Record<ListRowVariant, string> = {
    plain: 'w-full rounded-[calc(var(--panel-radius)*0.65)] text-left transition hover:bg-(--bg-surface-hover)',
    card: 'group w-full rounded-[calc(var(--panel-radius)*0.9)] border border-(--border-subtle) bg-(--bg-surface)/80 text-left transition-colors hover:bg-(--bg-surface-hover) shadow-(--shadow-sm)',
};

export const ListRow = React.forwardRef<HTMLButtonElement, ListRowProps>(({ variant = 'plain', className, type = 'button', ...props }, ref) => (
    <button
        ref={ref}
        type={type}
        className={cn(listRowVariantClassName[variant], className)}
        {...props}
    />
));

ListRow.displayName = 'ListRow';
