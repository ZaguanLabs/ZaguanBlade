import React from 'react';
import { cn } from '../../lib/utils';

type IconButtonSize = 'xs' | 'sm';
type IconButtonTone = 'neutral' | 'muted' | 'accent' | 'danger';

interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
    size?: IconButtonSize;
    tone?: IconButtonTone;
}

const iconButtonSizeClassName: Record<IconButtonSize, string> = {
    xs: 'p-0.5 rounded-(--radius-control)',
    sm: 'p-1 rounded-(--radius-control)',
};

const iconButtonToneClassName: Record<IconButtonTone, string> = {
    neutral: 'text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--row-hover)',
    muted: 'text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--row-hover)',
    accent: 'text-(--accent-ai) hover:text-(--accent-warning) hover:bg-(--surface-pane)',
    danger: 'text-(--fg-tertiary) hover:text-(--state-danger) hover:bg-[color-mix(in_srgb,var(--state-danger)_10%,transparent)]',
};

export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(({
    size = 'sm',
    tone = 'neutral',
    className,
    type = 'button',
    title,
    'aria-label': ariaLabel,
    'aria-labelledby': ariaLabelledBy,
    ...props
}, ref) => (
    <button
        ref={ref}
        type={type}
        title={title}
        aria-label={ariaLabel ?? (!ariaLabelledBy && typeof title === 'string' ? title : undefined)}
        aria-labelledby={ariaLabelledBy}
        className={cn(iconButtonSizeClassName[size], iconButtonToneClassName[tone], 'transition-colors', className)}
        {...props}
    />
));

IconButton.displayName = 'IconButton';
