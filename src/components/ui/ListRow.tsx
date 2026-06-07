import React from 'react';
import { cn } from '../../lib/utils';

type ListRowVariant = 'plain' | 'card';

interface ListRowProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
    variant?: ListRowVariant;
}

const listRowVariantClassName: Record<ListRowVariant, string> = {
    plain: 'w-full rounded-(--radius-control) text-left transition hover:bg-(--row-hover)',
    card: 'group w-full rounded-(--radius-control) border border-(--separator-subtle) bg-(--surface-overlay)/80 text-left transition-colors hover:bg-(--row-hover)',
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
