import { useEffect, useRef } from 'react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { CaseSensitive, ChevronDown, ChevronUp, Regex, Search, WholeWord, X } from 'lucide-react';

export interface TerminalSearchOptions {
    caseSensitive: boolean;
    wholeWord: boolean;
    regex: boolean;
}

interface TerminalFindWidgetProps {
    query: string;
    options: TerminalSearchOptions;
    matched: boolean | null;
    onQueryChange: (query: string) => void;
    onOptionsChange: (options: TerminalSearchOptions) => void;
    onNext: () => void;
    onPrevious: () => void;
    onClose: () => void;
}

function ToggleButton({
    active,
    label,
    onClick,
    children,
}: {
    active: boolean;
    label: string;
    onClick: () => void;
    children: ReactNode;
}) {
    return (
        <button
            type="button"
            aria-label={label}
            aria-pressed={active}
            title={label}
            onClick={onClick}
            className={`flex h-7 w-7 shrink-0 items-center justify-center rounded border transition-colors ${active
                ? 'border-(--accent-primary) bg-[color-mix(in_srgb,var(--accent-primary)_14%,transparent)] text-(--accent-primary)'
                : 'border-transparent text-(--fg-tertiary) hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)'
            }`}
        >
            {children}
        </button>
    );
}

export function TerminalFindWidget({
    query,
    options,
    matched,
    onQueryChange,
    onOptionsChange,
    onNext,
    onPrevious,
    onClose,
}: TerminalFindWidgetProps) {
    const { t } = useTranslation();
    const inputRef = useRef<HTMLInputElement>(null);

    useEffect(() => {
        inputRef.current?.focus();
        inputRef.current?.select();
    }, []);

    const updateOption = (key: keyof TerminalSearchOptions) => {
        onOptionsChange({
            ...options,
            [key]: !options[key],
        });
    };

    const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
        if (event.key === 'Escape') {
            event.preventDefault();
            onClose();
            return;
        }

        if (event.key === 'Enter') {
            event.preventDefault();
            if (event.shiftKey) {
                onPrevious();
            } else {
                onNext();
            }
        }
    };

    return (
        <div
            className="absolute right-3 top-2 z-20 flex max-w-[min(34rem,calc(100%-1.5rem))] items-center gap-1 rounded-md border border-(--border-default) bg-(--bg-surface) px-2 py-1 shadow-lg"
            role="search"
            aria-label={t('terminal.findInTerminal')}
        >
            <Search className="h-4 w-4 shrink-0 text-(--fg-tertiary)" aria-hidden="true" />
            <input
                ref={inputRef}
                value={query}
                onChange={(event) => onQueryChange(event.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={t('terminal.findPlaceholder')}
                className="h-7 min-w-0 w-48 bg-transparent text-sm text-(--fg-primary) outline-none placeholder:text-(--fg-tertiary)"
            />
            {query && matched === false && (
                <span className="hidden shrink-0 text-xs text-(--state-danger) sm:inline">
                    {t('terminal.findNoMatches')}
                </span>
            )}
            <button
                type="button"
                aria-label={t('terminal.findPrevious')}
                title={t('terminal.findPrevious')}
                onClick={onPrevious}
                className="flex h-7 w-7 shrink-0 items-center justify-center rounded text-(--fg-tertiary) transition-colors hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
            >
                <ChevronUp className="h-4 w-4" aria-hidden="true" />
            </button>
            <button
                type="button"
                aria-label={t('terminal.findNext')}
                title={t('terminal.findNext')}
                onClick={onNext}
                className="flex h-7 w-7 shrink-0 items-center justify-center rounded text-(--fg-tertiary) transition-colors hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
            >
                <ChevronDown className="h-4 w-4" aria-hidden="true" />
            </button>
            <div className="mx-1 h-5 w-px bg-(--border-subtle)" aria-hidden="true" />
            <ToggleButton
                active={options.caseSensitive}
                label={t('terminal.findMatchCase')}
                onClick={() => updateOption('caseSensitive')}
            >
                <CaseSensitive className="h-4 w-4" aria-hidden="true" />
            </ToggleButton>
            <ToggleButton
                active={options.wholeWord}
                label={t('terminal.findWholeWord')}
                onClick={() => updateOption('wholeWord')}
            >
                <WholeWord className="h-4 w-4" aria-hidden="true" />
            </ToggleButton>
            <ToggleButton
                active={options.regex}
                label={t('terminal.findRegex')}
                onClick={() => updateOption('regex')}
            >
                <Regex className="h-4 w-4" aria-hidden="true" />
            </ToggleButton>
            <button
                type="button"
                aria-label={t('terminal.findClose')}
                title={t('terminal.findClose')}
                onClick={onClose}
                className="flex h-7 w-7 shrink-0 items-center justify-center rounded text-(--fg-tertiary) transition-colors hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
            >
                <X className="h-4 w-4" aria-hidden="true" />
            </button>
        </div>
    );
}
