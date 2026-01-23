
import React, { useState, useEffect } from 'react';
import { StructureNode } from '../types/zlp';
import { ZLPService } from '../services/zlp';
import { ChevronRight, Box, FunctionSquare, Variable, Tag, FileText, Layout, Braces } from 'lucide-react';
import { useEditor } from '../contexts/EditorContext';

interface OutlinePanelProps {
    filePath: string | null;
    onNavigate: (path: string, line: number, character: number) => void;
}

// Icon mapping based on Tree-sitter node kinds
const SymbolIcon: React.FC<{ kind: string }> = ({ kind }) => {
    const k = kind.toLowerCase();

    // Functions/Methods
    if (k.includes('function') || k.includes('method') || k === 'constructor')
        return <FunctionSquare className="w-3.5 h-3.5 text-purple-400" />;

    // Classes/Structs/Interfaces
    if (k.includes('class') || k.includes('struct') || k.includes('interface') || k === 'impl' || k === 'trait')
        return <Layout className="w-3.5 h-3.5 text-orange-400" />;

    // Variables/Constants
    if (k.includes('const') || k.includes('let') || k.includes('var') || k === 'field')
        return <Variable className="w-3.5 h-3.5 text-blue-400" />;

    // Properties
    if (k.includes('property') || k === 'key')
        return <Tag className="w-3.5 h-3.5 text-sky-400" />;

    // Modules
    if (k.includes('module') || k === 'mod')
        return <Box className="w-3.5 h-3.5 text-yellow-400" />;

    // Misc Blocks
    if (k === 'object' || k === 'call_expression')
        return <Braces className="w-3.5 h-3.5 text-zinc-500" />;

    return <FileText className="w-3.5 h-3.5 text-zinc-600" />;
};

const OutlineItem: React.FC<{
    node: StructureNode;
    depth: number;
    onNavigate: (path: string, line: number, character: number) => void;
    filePath: string;
}> = ({ node, depth, onNavigate, filePath }) => {
    const [expanded, setExpanded] = useState(true);
    const hasChildren = node.children && node.children.length > 0;

    const handleClick = (e: React.MouseEvent) => {
        e.stopPropagation();
        onNavigate(filePath, node.selectionRange.start.line, node.selectionRange.start.column);
    };

    const toggleExpand = (e: React.MouseEvent) => {
        e.stopPropagation();
        setExpanded(!expanded);
    };

    return (
        <div className="select-none">
            <div
                className="flex items-center gap-1.5 py-0.5 px-2 hover:bg-[var(--bg-surface-hover)] cursor-pointer text-xs group"
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

                <SymbolIcon kind={node.kind} />
                <span className="truncate text-[var(--fg-secondary)] group-hover:text-[var(--fg-primary)] transition-colors">
                    {node.name}
                </span>
                {node.detail && (
                    <span className="ml-auto text-[10px] text-[var(--fg-tertiary)] opacity-0 group-hover:opacity-100 transition-opacity">
                        {node.detail}
                    </span>
                )}
            </div>

            {expanded && hasChildren && (
                <div>
                    {node.children!.map((child, idx) => (
                        <OutlineItem
                            key={`${child.name}-${idx}`}
                            node={child}
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
    const [structure, setStructure] = useState<StructureNode[]>([]);
    const [loading, setLoading] = useState(false);

    // We utilize the editorState simply to trigger updates if we want to hook into content changes later,
    // but primarily we refetch when filePath changes.
    // Ideally, we would also subscribe to 'save' events or file content changes.

    useEffect(() => {
        if (!filePath) {
            setStructure([]);
            return;
        }

        let isMounted = true;
        setLoading(true);

        const fetchStructure = async () => {
            // Artificial delay to prevent flicker on rapid tab switching
            await new Promise(r => setTimeout(r, 100));
            if (!isMounted) return;

            try {
                // For now, we pass empty string as content because zcoderd generally reads from disk
                // ZLP spec says we *can* send content if dirty, but let's assume saved file support first for simplicity.
                // In Phase 3 (Real-time), we will pipe the active dirty content.
                const nodes = await ZLPService.getStructure(filePath, "");

                if (isMounted) {
                    setStructure(nodes || []);
                }
            } catch (e) {
                console.warn("[Outline] Failed to fetch structure", e);
                // Fallback or empty
                if (isMounted) setStructure([]);
            } finally {
                if (isMounted) setLoading(false);
            }
        };

        fetchStructure();

        return () => { isMounted = false; };
    }, [filePath]);

    if (!filePath) return null;

    if (loading) {
        return (
            <div className="flex items-center justify-center p-8 text-[var(--fg-tertiary)]">
                <div className="animate-pulse text-xs italic">Parsing structure...</div>
            </div>
        );
    }

    if (structure.length === 0) {
        return (
            <div className="p-4 text-[10px] text-[var(--fg-tertiary)] italic text-center">
                No symbols found.
            </div>
        );
    }

    return (
        <div className="flex flex-col h-full bg-[var(--bg-panel)] overflow-y-auto pt-2 pb-4 scrollbar-thin scrollbar-thumb-zinc-800">
            {structure.map((node, idx) => (
                <OutlineItem
                    key={`${node.name}-${idx}`}
                    node={node}
                    depth={0}
                    onNavigate={onNavigate}
                    filePath={filePath}
                />
            ))}
        </div>
    );
};
