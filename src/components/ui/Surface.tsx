import React from 'react';
import { cn } from '../../lib/utils';

type SurfaceVariant = 'card' | 'elevated' | 'danger' | 'modal';

interface SurfaceProps extends React.HTMLAttributes<HTMLDivElement> {
    variant?: SurfaceVariant;
}

const surfaceVariantClassName: Record<SurfaceVariant, string> = {
    card: 'rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface)',
    elevated: 'rounded-[calc(var(--panel-radius)*1.25)] border border-(--border-subtle) bg-(--bg-surface)/80 shadow-(--shadow-xl)',
    danger: 'rounded-[calc(var(--panel-radius)*1.25)] border border-(--state-danger)/20 bg-[color-mix(in_srgb,var(--state-danger)_5%,transparent)] shadow-(--shadow-xl)',
    modal: 'rounded-(--panel-radius) border border-(--border-focus) bg-(--bg-surface) shadow-(--shadow-xl)',
};

export const Surface = React.forwardRef<HTMLDivElement, SurfaceProps>(({ variant = 'card', className, ...props }, ref) => (
    <div
        ref={ref}
        className={cn(surfaceVariantClassName[variant], className)}
        {...props}
    />
));

Surface.displayName = 'Surface';
