import React from 'react';
import { cn } from '../../lib/utils';

type IconButtonSize = 'xs' | 'sm';
type IconButtonTone = 'neutral' | 'muted' | 'accent' | 'danger';

interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
    size?: IconButtonSize;
    tone?: IconButtonTone;
}

const iconButtonSizeClassName: Record<IconButtonSize, string> = {
    xs: 'p-0.5 rounded',
    sm: 'p-1 rounded-[calc(var(--panel-radius)*0.35)]',
};

const iconButtonToneClassName: Record<IconButtonTone, string> = {
    neutral: 'text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover)',
    muted: 'text-zinc-500 hover:text-(--fg-primary) hover:bg-(--bg-surface-hover)',
    accent: 'text-(--accent-ai) hover:text-(--accent-warning) hover:bg-(--bg-app)',
    danger: 'text-zinc-500 hover:text-red-400 hover:bg-red-500/10',
};

export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(({ size = 'sm', tone = 'neutral', className, type = 'button', ...props }, ref) => (
    <button
        ref={ref}
        type={type}
        className={cn(iconButtonSizeClassName[size], iconButtonToneClassName[tone], 'transition-colors', className)}
        {...props}
    />
));

IconButton.displayName = 'IconButton';
