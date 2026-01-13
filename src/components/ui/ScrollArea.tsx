import React from 'react';
import { cn } from '@/lib/utils';

interface ScrollAreaProps extends React.HTMLAttributes<HTMLDivElement> {
    orientation?: 'vertical' | 'horizontal';
}

export const ScrollArea = React.forwardRef<HTMLDivElement, ScrollAreaProps>(
    ({ className, orientation = 'vertical', children, ...props }, ref) => {
        return (
            <div
                ref={ref}
                className={cn(
                    'relative overflow-auto',
                    orientation === 'horizontal' ? 'overflow-x-auto overflow-y-hidden' : 'overflow-x-hidden overflow-y-auto',
                    className
                )}
                {...props}
            >
                {children}
            </div>
        );
    }
);
ScrollArea.displayName = 'ScrollArea';
