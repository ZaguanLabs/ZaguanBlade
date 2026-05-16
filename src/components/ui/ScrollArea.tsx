import React from 'react';
import { useSmoothWheelScroll } from '../../hooks/useSmoothWheelScroll';
import { cn } from '../../lib/utils';

type ScrollAreaProps = React.HTMLAttributes<HTMLDivElement>;

export const ScrollArea = React.forwardRef<HTMLDivElement, ScrollAreaProps>(({ className, onWheel, ...props }, ref) => {
    const handleWheel = useSmoothWheelScroll<HTMLDivElement>(onWheel);

    return (
        <div
            ref={ref}
            className={cn('overflow-y-auto', className)}
            onWheel={handleWheel}
            {...props}
        />
    );
});

ScrollArea.displayName = 'ScrollArea';
