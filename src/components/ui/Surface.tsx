import React from 'react';
import { cn } from '../../lib/utils';

type SurfaceVariant = 'card' | 'row' | 'elevated' | 'danger' | 'modal';

interface SurfaceProps extends React.HTMLAttributes<HTMLDivElement> {
    variant?: SurfaceVariant;
}

const surfaceVariantClassName: Record<SurfaceVariant, string> = {
    card: 'rounded-(--radius-control) border border-(--separator-subtle) bg-(--surface-overlay)',
    row: 'rounded-(--radius-control) border border-(--separator-subtle) bg-(--surface-overlay)',
    elevated: 'rounded-(--radius-popover) border border-(--separator-subtle) bg-(--surface-overlay)/80 shadow-(--shadow-popover)',
    danger: 'rounded-(--radius-popover) border border-(--state-danger)/20 bg-[color-mix(in_srgb,var(--state-danger)_5%,transparent)] shadow-(--shadow-popover)',
    modal: 'rounded-(--radius-dialog) border border-(--focus-ring) bg-(--surface-overlay) shadow-(--shadow-dialog)',
};

export const Surface = React.forwardRef<HTMLDivElement, SurfaceProps>(({ variant = 'card', className, ...props }, ref) => (
    <div
        ref={ref}
        className={cn(surfaceVariantClassName[variant], className)}
        {...props}
    />
));

Surface.displayName = 'Surface';
