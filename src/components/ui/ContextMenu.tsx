import React, { useEffect, useRef, useState, createContext, useContext, useCallback } from 'react';

/**
 * Context Menu System
 * 
 * A reusable, portal-based context menu component with:
 * - Proper positioning that stays within viewport
 * - Keyboard navigation (arrow keys, escape, enter)
 * - Submenus support
 * - Dividers and disabled items
 * - Beautiful animations
 */

export interface ContextMenuItem {
    id: string;
    label: string;
    icon?: React.ReactNode;
    shortcut?: string;
    disabled?: boolean;
    danger?: boolean;
    divider?: boolean;
    submenu?: ContextMenuItem[];
    onClick?: () => void;
}

interface ContextMenuPosition {
    x: number;
    y: number;
}

interface ContextMenuState {
    isOpen: boolean;
    position: ContextMenuPosition;
    items: ContextMenuItem[];
    context?: unknown;
}

interface ContextMenuContextType {
    showMenu: (position: ContextMenuPosition, items: ContextMenuItem[], context?: unknown) => void;
    hideMenu: () => void;
    state: ContextMenuState;
}

const ContextMenuContext = createContext<ContextMenuContextType | null>(null);

export const useContextMenu = () => {
    const context = useContext(ContextMenuContext);
    if (!context) {
        throw new Error('useContextMenu must be used within a ContextMenuProvider');
    }
    return context;
};

/**
 * Provider component that enables context menus throughout the app
 */
export const ContextMenuProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [state, setState] = useState<ContextMenuState>({
        isOpen: false,
        position: { x: 0, y: 0 },
        items: [],
        context: undefined,
    });

    const showMenu = useCallback((position: ContextMenuPosition, items: ContextMenuItem[], context?: unknown) => {
        setState({
            isOpen: true,
            position,
            items,
            context,
        });
    }, []);

    const hideMenu = useCallback(() => {
        setState(prev => ({ ...prev, isOpen: false }));
    }, []);

    return (
        <ContextMenuContext.Provider value={{ showMenu, hideMenu, state }}>
            {children}
            {state.isOpen && <ContextMenuPortal />}
        </ContextMenuContext.Provider>
    );
};

/**
 * The actual menu portal that renders at the root level
 */
const ContextMenuPortal: React.FC = () => {
    const { state, hideMenu } = useContextMenu();
    const menuRef = useRef<HTMLDivElement>(null);
    const [adjustedPosition, setAdjustedPosition] = useState(state.position);
    const [activeIndex, setActiveIndex] = useState(-1);

    // Adjust position to stay within viewport
    useEffect(() => {
        if (menuRef.current) {
            const rect = menuRef.current.getBoundingClientRect();
            let { x, y } = state.position;

            // Adjust horizontal position
            if (x + rect.width > window.innerWidth - 8) {
                x = window.innerWidth - rect.width - 8;
            }

            // Adjust vertical position
            if (y + rect.height > window.innerHeight - 8) {
                y = window.innerHeight - rect.height - 8;
            }

            setAdjustedPosition({ x: Math.max(8, x), y: Math.max(8, y) });
        }
    }, [state.position, state.items]);

    // Close on click outside
    useEffect(() => {
        const handleClickOutside = (e: MouseEvent) => {
            if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
                hideMenu();
            }
        };

        // Close on any click (including inside menu items that trigger actions)
        document.addEventListener('mousedown', handleClickOutside);
        return () => document.removeEventListener('mousedown', handleClickOutside);
    }, [hideMenu]);

    // Keyboard navigation
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            const enabledItems = state.items.filter(item => !item.divider && !item.disabled);

            switch (e.key) {
                case 'Escape':
                    hideMenu();
                    break;
                case 'ArrowDown':
                    e.preventDefault();
                    setActiveIndex(prev => {
                        const next = prev + 1;
                        return next >= enabledItems.length ? 0 : next;
                    });
                    break;
                case 'ArrowUp':
                    e.preventDefault();
                    setActiveIndex(prev => {
                        const next = prev - 1;
                        return next < 0 ? enabledItems.length - 1 : next;
                    });
                    break;
                case 'Enter':
                    e.preventDefault();
                    if (activeIndex >= 0 && activeIndex < enabledItems.length) {
                        const item = enabledItems[activeIndex];
                        if (item.onClick) {
                            item.onClick();
                            hideMenu();
                        }
                    }
                    break;
            }
        };

        document.addEventListener('keydown', handleKeyDown);
        return () => document.removeEventListener('keydown', handleKeyDown);
    }, [state.items, activeIndex, hideMenu]);

    const handleItemClick = (item: ContextMenuItem) => {
        if (item.disabled || item.divider) return;
        if (item.onClick) {
            item.onClick();
        }
        hideMenu();
    };

    let enabledIndex = -1;

    return (
        <div
            ref={menuRef}
            className="fixed z-[9999] min-w-[180px] max-w-[280px] py-1.5 bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded-lg shadow-xl animate-in fade-in zoom-in-95 duration-100"
            style={{
                left: adjustedPosition.x,
                top: adjustedPosition.y,
                boxShadow: '0 8px 30px rgba(0, 0, 0, 0.4), 0 0 1px rgba(255, 255, 255, 0.1)',
            }}
        >
            {state.items.map((item, index) => {
                if (item.divider) {
                    return (
                        <div
                            key={`divider-${index}`}
                            className="my-1.5 mx-2 h-px bg-[var(--border-subtle)]"
                        />
                    );
                }

                if (!item.disabled) enabledIndex++;
                const isActive = enabledIndex === activeIndex;

                return (
                    <button
                        key={item.id}
                        onClick={() => handleItemClick(item)}
                        disabled={item.disabled}
                        className={`
                            w-full flex items-center gap-3 px-3 py-1.5 text-left text-[13px] transition-colors
                            ${item.disabled
                                ? 'text-[var(--fg-tertiary)] cursor-not-allowed'
                                : item.danger
                                    ? 'text-[var(--accent-error)] hover:bg-[var(--accent-error)]/10'
                                    : 'text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)]'
                            }
                            ${isActive && !item.disabled ? 'bg-[var(--bg-surface-hover)]' : ''}
                        `}
                    >
                        {/* Icon */}
                        {item.icon && (
                            <span className="w-4 h-4 flex items-center justify-center opacity-70">
                                {item.icon}
                            </span>
                        )}

                        {/* Label */}
                        <span className="flex-1">{item.label}</span>

                        {/* Shortcut */}
                        {item.shortcut && (
                            <span className="text-[11px] text-[var(--fg-tertiary)] font-mono">
                                {item.shortcut}
                            </span>
                        )}
                    </button>
                );
            })}
        </div>
    );
};

/**
 * Hook to easily add context menu to any element
 */
export const useContextMenuTrigger = (items: ContextMenuItem[] | (() => ContextMenuItem[]), context?: unknown) => {
    const { showMenu } = useContextMenu();

    const handleContextMenu = useCallback((e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();

        const menuItems = typeof items === 'function' ? items() : items;
        showMenu({ x: e.clientX, y: e.clientY }, menuItems, context);
    }, [items, context, showMenu]);

    return { onContextMenu: handleContextMenu };
};
