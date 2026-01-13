import React from 'react';
import { cn } from '@/lib/utils';

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger' | 'outline';
    size?: 'sm' | 'md' | 'lg' | 'icon';
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
    ({ className, variant = 'primary', size = 'md', ...props }, ref) => {
        return (
            <button
                ref={ref}
                className={cn(
                    'inline-flex items-center justify-center rounded font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-[var(--border-focus)] disabled:opacity-50 disabled:pointer-events-none',
                    {
                        // Variants
                        'bg-[var(--accent)] text-white hover:opacity-90': variant === 'primary',
                        'bg-[var(--bg-surface-hover)] text-[var(--fg-primary)] hover:bg-[var(--border-focus)]': variant === 'secondary',
                        'bg-transparent text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)]': variant === 'ghost',
                        'bg-red-500/10 text-red-500 hover:bg-red-500/20': variant === 'danger',
                        'border border-[var(--border-subtle)] bg-transparent hover:bg-[var(--bg-surface-hover)]': variant === 'outline',

                        // Sizes
                        'h-8 px-3 text-xs': size === 'sm',
                        'h-10 px-4 text-sm': size === 'md',
                        'h-12 px-6 text-base': size === 'lg',
                        'h-8 w-8 p-0': size === 'icon',
                    },
                    className
                )}
                {...props}
            />
        );
    }
);
Button.displayName = 'Button';
