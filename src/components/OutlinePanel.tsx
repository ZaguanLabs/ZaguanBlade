
import React, { useState, useEffect } from 'react';
import { LanguageDocumentSymbol } from '../types/blade';
import { LanguageService } from '../services/language';
import { ChevronRight, Box, FunctionSquare, Variable, Tag } from 'lucide-react';
import { useEditor } from '../contexts/EditorContext';

interface OutlinePanelProps {
    filePath: string | null;
    onNavigate: (path: string, line: number, character: number) => void;
}

const SymbolIcon: React.FC<{ kind: string }> = ({ kind }) => {
    // Map LSP symbol kinds (roughly) to icons
    // See LSP spec for kind mappings: 
    // 1: File, 2: Module, 3: Namespace, 4: Package, 5: Class, 6: Method, 
    // 7: Property, 8: Field, 9: Constructor, 10: Enum, 11: Interface, 
    // 12: Function, 13: Variable, 14: Constant, 15: String, 16: Number, 
    // 17: Boolean, 18: Array, 19: Object, 20: Key, 21: Null, 22: EnumMember, 
    // 23: Struct, 24: Event, 25: Operator, 26: TypeParameter

    // Note: The Rust backend might be returning string representations or numbers.
    // The types/blade.ts defines kind as string.

    const k = kind.toLowerCase();

    if (k.includes('function') || k.includes('method') || k === '12' || k === '6')
        return <FunctionSquare className="w-3.5 h-3.5 text-purple-400" />;

    if (k.includes('class') || k.includes('struct') || k.includes('interface') || k === '5' || k === '11' || k === '23')
        return <Box className="w-3.5 h-3.5 text-orange-400" />;

    if (k.includes('variable') || k.includes('constant') || k === '13' || k === '14')
        return <Variable className="w-3.5 h-3.5 text-blue-400" />;

    if (k.includes('property') || k.includes('field') || k === '7' || k === '8')
        return <Tag className="w-3.5 h-3.5 text-sky-400" />;

    return <Box className="w-3.5 h-3.5 text-zinc-500" />;
};

const OutlineItem: React.FC<{
    symbol: LanguageDocumentSymbol;
    depth: number;
    onNavigate: (path: string, line: number, character: number) => void;
    filePath: string;
}> = ({ symbol, depth, onNavigate, filePath }) => {
    const [expanded, setExpanded] = useState(true);
    const hasChildren = symbol.children && symbol.children.length > 0;

    const handleClick = (e: React.MouseEvent) => {
        e.stopPropagation();
        onNavigate(filePath, symbol.selection_range.start.line, symbol.selection_range.start.character);
    };

    const toggleExpand = (e: React.MouseEvent) => {
        e.stopPropagation();
        setExpanded(!expanded);
    };

    return (
        <div className="select-none">
            <div
                className="flex items-center gap-1.5 py-0.5 px-2 hover:bg-[var(--bg-surface-hover)] cursor-pointer text-xs"
                style={{ paddingLeft: `${depth * 16 + 8}px` }}
                onClick={handleClick}
            >
                {hasChildren ? (
                    <span
                        className={`transition-transform cursor-pointer hover:text-[var(--fg-primary)] p-0.5 rounded-sm ${expanded ? 'rotate-90' : ''}`}
                        onClick={toggleExpand}
                    >
                        <ChevronRight className="w-3 h-3 text-[var(--fg-tertiary)]" />
                    </span>
                ) : (
                    <span className="w-4" />
                )}

                <SymbolIcon kind={symbol.kind} />
                <span className="truncate text-[var(--fg-secondary)]">{symbol.name}</span>
            </div>

            {expanded && hasChildren && (
                <div>
                    {symbol.children.map((child, idx) => (
                        <OutlineItem
                            key={`${child.name}-${idx}`}
                            symbol={child}
                            depth={depth + 1}
                            onNavigate={onNavigate}
                            filePath={filePath}
                        />
                    ))}
                </div>
            )}
        </div>
    );
};

export const OutlinePanel: React.FC<OutlinePanelProps> = ({ filePath, onNavigate }) => {
    const [symbols, setSymbols] = useState<LanguageDocumentSymbol[]>([]);
    const [loading, setLoading] = useState(false);
    const { editorState } = useEditor();
    const { enableLsp } = editorState;

    useEffect(() => {
        if (!filePath || !enableLsp) {
            setSymbols([]);
            return;
        }

        let isMounted = true;
        setLoading(true);

        const fetchSymbols = async () => {
            // Artificial delay to prevent weird flickering on fast switches
            // and to allow file to be indexed if just opened
            await new Promise(r => setTimeout(r, 200));

            if (!isMounted) return;

            try {
                const result = await LanguageService.getDocumentSymbols(filePath);
                if (isMounted) {
                    setSymbols(result || []);
                }
            } catch (e) {
                console.warn("[Outline] Failed to fetch symbols", e);
            } finally {
                if (isMounted) setLoading(false);
            }
        };

        fetchSymbols();

        return () => { isMounted = false; };
    }, [filePath, enableLsp]);

    if (!filePath) return null;

    if (!enableLsp) {
        return (
            <div className="p-4 text-[10px] text-[var(--fg-tertiary)] italic text-center">
                Language Intelligence is disabled. Enable it in settings to see the outline.
            </div>
        );
    }

    if (loading) {
        return (
            <div className="p-4 text-[10px] text-[var(--fg-tertiary)] italic text-center">
                Loading outline...
            </div>
        );
    }

    if (symbols.length === 0) {
        return (
            <div className="p-4 text-[10px] text-[var(--fg-tertiary)] italic text-center">
                No symbols found.
            </div>
        );
    }

    return (
        <div className="flex flex-col h-full bg-[var(--bg-panel)] overflow-y-auto pt-2 pb-4 scrollbar-thin scrollbar-thumb-zinc-800">
            {symbols.map((symbol, idx) => (
                <OutlineItem
                    key={`${symbol.name}-${idx}`}
                    symbol={symbol}
                    depth={0}
                    onNavigate={onNavigate}
                    filePath={filePath}
                />
            ))}
        </div>
    );
};
