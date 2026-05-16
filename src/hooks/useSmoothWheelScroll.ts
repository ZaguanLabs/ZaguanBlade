import { useCallback, useEffect, useRef, type WheelEvent as ReactWheelEvent } from 'react';

const LINE_HEIGHT_PX = 16;
const SETTLE_THRESHOLD_PX = 0.5;
const WHEEL_EASING = 0.24;

type WheelHandler<T extends HTMLElement> = (event: ReactWheelEvent<T>) => void;
type SmoothWheelOptions = {
    disabled?: boolean;
    resetKey?: unknown;
};
type LegacyMediaQueryList = MediaQueryList & {
    addListener?: (listener: (event: MediaQueryListEvent) => void) => void;
    removeListener?: (listener: (event: MediaQueryListEvent) => void) => void;
};

function clamp(value: number, min: number, max: number) {
    return Math.min(max, Math.max(min, value));
}

function normalizeWheelDelta(event: ReactWheelEvent<HTMLElement>, element: HTMLElement) {
    if (event.deltaMode === window.WheelEvent.DOM_DELTA_LINE) {
        return event.deltaY * LINE_HEIGHT_PX;
    }

    if (event.deltaMode === window.WheelEvent.DOM_DELTA_PAGE) {
        return event.deltaY * element.clientHeight * 0.9;
    }

    return event.deltaY;
}

function canScrollVertically(element: HTMLElement, deltaY: number) {
    const maxScrollTop = Math.max(0, element.scrollHeight - element.clientHeight);
    if (maxScrollTop <= 0) {
        return false;
    }

    if (deltaY < 0 && element.scrollTop <= 0) {
        return false;
    }

    if (deltaY > 0 && element.scrollTop >= maxScrollTop) {
        return false;
    }

    return true;
}

function hasNestedScrollable(target: EventTarget | null, root: HTMLElement, deltaY: number) {
    let element = target instanceof HTMLElement ? target : null;

    while (element && element !== root) {
        const style = window.getComputedStyle(element);
        const canOverflow = style.overflowY === 'auto' || style.overflowY === 'scroll';

        if (canOverflow && canScrollVertically(element, deltaY)) {
            return true;
        }

        element = element.parentElement;
    }

    return false;
}

export function useSmoothWheelScroll<T extends HTMLElement>(onWheel?: WheelHandler<T>, options: SmoothWheelOptions = {}): WheelHandler<T> {
    const frameRef = useRef<number | null>(null);
    const targetScrollTopRef = useRef(0);
    const reduceMotionRef = useRef(false);

    const stopAnimation = useCallback(() => {
        if (frameRef.current !== null) {
            cancelAnimationFrame(frameRef.current);
            frameRef.current = null;
        }
    }, []);

    useEffect(() => {
        const mediaQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
        const updatePreference = () => {
            reduceMotionRef.current = mediaQuery.matches;
        };

        updatePreference();
        const legacyMediaQuery = mediaQuery as LegacyMediaQueryList;

        if (typeof mediaQuery.addEventListener === 'function') {
            mediaQuery.addEventListener('change', updatePreference);
        } else {
            legacyMediaQuery.addListener?.(updatePreference);
        }

        return () => {
            if (typeof mediaQuery.removeEventListener === 'function') {
                mediaQuery.removeEventListener('change', updatePreference);
            } else {
                legacyMediaQuery.removeListener?.(updatePreference);
            }
            stopAnimation();
        };
    }, [stopAnimation]);

    useEffect(() => {
        stopAnimation();
    }, [options.resetKey, stopAnimation]);

    const animate = useCallback((element: T) => {
        const step = () => {
            const current = element.scrollTop;
            const target = targetScrollTopRef.current;
            const distance = target - current;

            if (Math.abs(distance) <= SETTLE_THRESHOLD_PX) {
                element.scrollTop = target;
                frameRef.current = null;
                return;
            }

            element.scrollTop = current + distance * WHEEL_EASING;
            frameRef.current = requestAnimationFrame(step);
        };

        frameRef.current = requestAnimationFrame(step);
    }, []);

    return useCallback((event: ReactWheelEvent<T>) => {
        onWheel?.(event);

        if (event.defaultPrevented || options.disabled || reduceMotionRef.current || event.ctrlKey || event.metaKey) {
            return;
        }

        if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) {
            return;
        }

        const element = event.currentTarget;
        const deltaY = normalizeWheelDelta(event, element);

        if (!canScrollVertically(element, deltaY) || hasNestedScrollable(event.target, element, deltaY)) {
            return;
        }

        event.preventDefault();

        const maxScrollTop = Math.max(0, element.scrollHeight - element.clientHeight);
        const baseScrollTop = frameRef.current === null ? element.scrollTop : targetScrollTopRef.current;
        targetScrollTopRef.current = clamp(baseScrollTop + deltaY, 0, maxScrollTop);

        if (frameRef.current === null) {
            animate(element);
        }
    }, [animate, onWheel, options.disabled]);
}
