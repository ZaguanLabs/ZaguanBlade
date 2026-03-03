import { useState, useEffect, useCallback, RefObject } from 'react';

interface UseResizeHandlersOptions {
    editorColumnRef: RefObject<HTMLDivElement | null>;
    initialTerminalHeight?: number;
    initialChatPanelWidth?: number;
}

interface ResizeHandlers {
    terminalHeight: number;
    setTerminalHeight: (h: number | ((prev: number) => number)) => void;
    chatPanelWidth: number;
    setChatPanelWidth: (w: number | ((prev: number) => number)) => void;
    isTerminalDragging: boolean;
    isChatDragging: boolean;
    handleTerminalMouseDown: (e: React.MouseEvent) => void;
    handleChatMouseDown: (e: React.MouseEvent) => void;
}

export function useResizeHandlers({
    editorColumnRef,
    initialTerminalHeight = 300,
    initialChatPanelWidth = 400,
}: UseResizeHandlersOptions): ResizeHandlers {
    const [terminalHeight, setTerminalHeight] = useState(initialTerminalHeight);
    const [chatPanelWidth, setChatPanelWidth] = useState(initialChatPanelWidth);
    const [isTerminalDragging, setIsTerminalDragging] = useState(false);
    const [isChatDragging, setIsChatDragging] = useState(false);

    // Terminal panel resize handler
    const handleTerminalMouseDown = useCallback((e: React.MouseEvent) => {
        if (e.button !== 0) return;
        setIsTerminalDragging(true);
        e.preventDefault();
    }, []);

    useEffect(() => {
        const handleTerminalMouseMove = (e: MouseEvent) => {
            if (!isTerminalDragging) return;
            const col = editorColumnRef.current;
            if (!col) return;
            const rect = col.getBoundingClientRect();
            const newHeight = rect.bottom - e.clientY;
            if (newHeight >= 80 && newHeight <= rect.height - 60) {
                setTerminalHeight(newHeight);
            }
        };

        const handleTerminalMouseUp = () => {
            setIsTerminalDragging(false);
        };

        if (isTerminalDragging) {
            document.addEventListener('mousemove', handleTerminalMouseMove);
            document.addEventListener('mouseup', handleTerminalMouseUp);
            document.body.classList.add('resize-y-cursor');
        }

        return () => {
            document.removeEventListener('mousemove', handleTerminalMouseMove);
            document.removeEventListener('mouseup', handleTerminalMouseUp);
            document.body.classList.remove('resize-y-cursor');
        };
    }, [isTerminalDragging, editorColumnRef]);

    // Chat panel resize handler
    const handleChatMouseDown = useCallback((e: React.MouseEvent) => {
        if (e.button !== 0) return;
        setIsChatDragging(true);
        e.preventDefault();
    }, []);

    useEffect(() => {
        const handleChatMouseMove = (e: MouseEvent) => {
            if (!isChatDragging) return;
            const newWidth = window.innerWidth - e.clientX;
            if (newWidth >= 280 && newWidth <= 800) {
                setChatPanelWidth(newWidth);
            }
        };

        const handleChatMouseUp = () => {
            setIsChatDragging(false);
        };

        if (isChatDragging) {
            document.addEventListener('mousemove', handleChatMouseMove);
            document.addEventListener('mouseup', handleChatMouseUp);
            document.body.classList.add('resize-x-cursor');
        }

        return () => {
            document.removeEventListener('mousemove', handleChatMouseMove);
            document.removeEventListener('mouseup', handleChatMouseUp);
            document.body.classList.remove('resize-x-cursor');
        };
    }, [isChatDragging]);

    return {
        terminalHeight,
        setTerminalHeight,
        chatPanelWidth,
        setChatPanelWidth,
        isTerminalDragging,
        isChatDragging,
        handleTerminalMouseDown,
        handleChatMouseDown,
    };
}
