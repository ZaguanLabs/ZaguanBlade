'use client';
import React, { useState, useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { ToolCall } from '../types/chat';
import { Zap, CheckCircle2, XCircle, Loader2, Copy, Check, ChevronRight, ChevronDown, RotateCcw, StopCircle, FileSearch, ShieldAlert, GitBranch } from 'lucide-react';

const COMPLETE_FADE_DELAY_MS = 250;
const COMPLETE_VISUAL_HOLD_MS = 1100;

interface ToolCallDisplayProps {
    toolCall: ToolCall;
    status?: 'pending' | 'executing' | 'complete' | 'error' | 'skipped';
    result?: string;
    onStopCommand?: () => void;
    onUndo?: () => void;
    onOpenFile?: (path: string) => void;
    workspaceRoot?: string | null;
}

type JsonRecord = Record<string, unknown>;

const asRecord = (value: unknown): JsonRecord | null => {
    return value && typeof value === 'object' && !Array.isArray(value) ? value as JsonRecord : null;
};

const asArray = (value: unknown): unknown[] => {
    return Array.isArray(value) ? value : [];
};

const asString = (value: unknown): string => {
    return typeof value === 'string' ? value : '';
};

const asNumber = (value: unknown): number | null => {
    return typeof value === 'number' && Number.isFinite(value) ? value : null;
};

const parseJsonRecord = (value?: string): JsonRecord | null => {
    if (!value) {
        return null;
    }
    try {
        return asRecord(JSON.parse(value));
    } catch {
        return null;
    }
};

// Symbols-family tools whose results carry a count in `_meta`. The count field
// varies per tool: graph tools emit `returned` (Track F), search/anchor/related
// emit `count`, and symbol_outline emits `returned_symbols`.
const isSymbolResultTool = (name: string): boolean =>
    name.startsWith('symbol_') || name === 'semantic_anchor_search';

const symbolResultCount = (jsonResult: JsonRecord | null): number | null => {
    const meta = asRecord(jsonResult?.['_meta']);
    if (!meta) {
        return null;
    }
    return asNumber(meta.returned) ?? asNumber(meta.count) ?? asNumber(meta.returned_symbols);
};

const toWorkspaceRelativePath = (path: string, workspaceRoot?: string | null): string => {
    if (!path || !workspaceRoot) {
        return path;
    }

    const normalizedPath = path.replace(/\\/g, '/');
    const normalizedRoot = workspaceRoot.replace(/\\/g, '/').replace(/\/+$/, '');
    if (!normalizedRoot) {
        return path;
    }

    if (normalizedPath === normalizedRoot) {
        return '';
    }

    const rootWithSlash = `${normalizedRoot}/`;
    if (normalizedPath.startsWith(rootWithSlash)) {
        return normalizedPath.slice(rootWithSlash.length);
    }

    return path;
};

const badgeClassForTone = (tone: string) => {
    switch (tone.toLowerCase()) {
        case 'high':
            return 'border-(--state-danger)/30 bg-[color-mix(in_srgb,var(--state-danger)_12%,transparent)] text-(--state-danger)';
        case 'medium':
            return 'border-(--accent-warning)/30 bg-[color-mix(in_srgb,var(--accent-warning)_12%,transparent)] text-(--accent-warning)';
        case 'low':
            return 'border-(--accent-mention)/30 bg-[color-mix(in_srgb,var(--accent-mention)_12%,transparent)] text-(--accent-mention)';
        default:
            return 'border-(--border-subtle) bg-(--bg-surface)/55 text-(--fg-secondary)';
    }
};

const CompactBadge: React.FC<{ label: string; tone?: string }> = ({ label, tone = '' }) => (
    <span className={`rounded-full border px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.12em] ${badgeClassForTone(tone)}`}>
        {label}
    </span>
);

const PathButton: React.FC<{ path: string; onOpenFile?: (path: string) => void; children?: React.ReactNode }> = ({ path, onOpenFile, children }) => (
    <button
        type="button"
        onClick={() => onOpenFile?.(path)}
        disabled={!onOpenFile}
        className={`min-w-0 truncate text-left font-mono text-[10px] ${onOpenFile ? 'text-(--accent-ai) hover:text-(--fg-primary)' : 'text-(--fg-secondary)'}`}
        title={path}
        aria-label={path}
    >
        {children || path}
    </button>
);

const SuggestedRangeLabel: React.FC<{ ranges: unknown[] }> = ({ ranges }) => {
    const first = asRecord(ranges[0]);
    const start = asNumber(first?.start_line);
    const end = asNumber(first?.end_line);
    if (!start || !end) {
        return null;
    }
    return <span className="text-[9px] text-(--fg-tertiary)">L{start}-{end}</span>;
};

const FastContextResultCard: React.FC<{
    payload: JsonRecord;
    statusText: string;
    statusIcon: React.ReactNode;
    isVisuallyComplete: boolean;
    onOpenFile?: (path: string) => void;
}> = ({ payload, statusText, statusIcon, isVisuallyComplete, onOpenFile }) => {
    const { t } = useTranslation();
    const primaryFiles = asArray(payload.primary_files).map(asRecord).filter(Boolean) as JsonRecord[];
    const relatedFiles = asArray(payload.related_files).map(asRecord).filter(Boolean) as JsonRecord[];
    const enrichedFiles = asArray(payload.enriched_files).map(asRecord).filter(Boolean) as JsonRecord[];
    const health = asRecord(payload.index_health);
    const summary = asString(payload.summary);
    const confidence = asString(payload.confidence);
    const recommendedNextStep = asString(payload.recommended_next_step);
    const healthStatus = asString(health?.status);

    return (
        <div className={`rounded-[calc(var(--panel-radius)*0.55)] border border-(--accent-ai)/20 bg-[color-mix(in_srgb,var(--accent-ai)_7%,transparent)] p-2.5 text-[11px] transition-opacity duration-1000 ease-out ${isVisuallyComplete ? 'opacity-70' : 'opacity-100'}`}>
            <div className="flex items-start gap-2">
                <div className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-(--accent-ai)/25 bg-(--bg-surface)/55">
                    <FileSearch className="h-3.5 w-3.5 text-(--accent-ai)" aria-hidden="true" />
                </div>
                <div className="min-w-0 flex-1 space-y-2">
                    <div className="flex flex-wrap items-center gap-1.5">
                        <span className="font-semibold text-(--fg-primary)">{t('toolCall.fastContext.title')}</span>
                        <CompactBadge label={confidence || t('toolCall.badges.unknown')} tone={confidence} />
                        {healthStatus && <CompactBadge label={t('toolCall.fastContext.indexBadge', { status: healthStatus })} tone={healthStatus === 'Fresh' ? 'low' : 'medium'} />}
                        <span className="ml-auto inline-flex items-center gap-1 text-[9px] font-semibold uppercase tracking-[0.14em] text-(--fg-tertiary)">
                            {statusIcon}
                            {statusText}
                        </span>
                    </div>
                    {summary && <div className="text-[11px] leading-4 text-(--fg-secondary)">{summary}</div>}
                    <div className="grid grid-cols-3 gap-1.5">
                        <div className="rounded border border-(--border-subtle) bg-(--bg-surface)/45 px-2 py-1">
                            <div className="text-[9px] uppercase tracking-[0.12em] text-(--fg-tertiary)">{t('toolCall.fastContext.primary')}</div>
                            <div className="text-sm font-semibold text-(--fg-primary)">{primaryFiles.length}</div>
                        </div>
                        <div className="rounded border border-(--border-subtle) bg-(--bg-surface)/45 px-2 py-1">
                            <div className="text-[9px] uppercase tracking-[0.12em] text-(--fg-tertiary)">{t('toolCall.fastContext.related')}</div>
                            <div className="text-sm font-semibold text-(--fg-primary)">{relatedFiles.length}</div>
                        </div>
                        <div className="rounded border border-(--border-subtle) bg-(--bg-surface)/45 px-2 py-1">
                            <div className="text-[9px] uppercase tracking-[0.12em] text-(--fg-tertiary)">{t('toolCall.fastContext.enriched')}</div>
                            <div className="text-sm font-semibold text-(--fg-primary)">{enrichedFiles.length}</div>
                        </div>
                    </div>
                    {primaryFiles.length > 0 && (
                        <div className="space-y-1">
                            {primaryFiles.slice(0, 4).map((file, index) => {
                                const path = asString(file.path);
                                const score = asNumber(file.score);
                                const ranges = asArray(file.suggested_ranges);
                                if (!path) {
                                    return null;
                                }
                                return (
                                    <div key={`${path}-${index}`} className="flex min-w-0 items-center gap-2 rounded border border-(--border-subtle) bg-(--bg-app)/35 px-2 py-1">
                                        <PathButton path={path} onOpenFile={onOpenFile} />
                                        <div className="ml-auto flex shrink-0 items-center gap-1.5">
                                            {score !== null && <span className="text-[9px] text-(--fg-tertiary)">{score}</span>}
                                            <SuggestedRangeLabel ranges={ranges} />
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    )}
                    {recommendedNextStep && (
                        <div className="rounded border border-(--accent-planning)/20 bg-[color-mix(in_srgb,var(--accent-planning)_7%,transparent)] px-2 py-1.5 text-[10px] leading-4 text-(--fg-secondary)">
                            {recommendedNextStep}
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
};

const EditImpactResultCard: React.FC<{
    payload: JsonRecord;
    statusText: string;
    statusIcon: React.ReactNode;
    isVisuallyComplete: boolean;
    onOpenFile?: (path: string) => void;
}> = ({ payload, statusText, statusIcon, isVisuallyComplete, onOpenFile }) => {
    const { t } = useTranslation();
    const impact = asRecord(payload.impact) || {};
    const impactedFiles = asArray(impact.impacted_files).map(asRecord).filter(Boolean) as JsonRecord[];
    const likelyTests = asArray(impact.likely_tests).map(asRecord).filter(Boolean) as JsonRecord[];
    const risk = asString(impact.risk);
    const confidence = asString(impact.confidence);
    const referenceCount = asNumber(impact.reference_count) ?? 0;
    const nextSteps = asArray(impact.recommended_next_steps).map(asString).filter(Boolean);

    return (
        <div className={`rounded-[calc(var(--panel-radius)*0.55)] border border-(--accent-warning)/24 bg-[color-mix(in_srgb,var(--accent-warning)_7%,transparent)] p-2.5 text-[11px] transition-opacity duration-1000 ease-out ${isVisuallyComplete ? 'opacity-70' : 'opacity-100'}`}>
            <div className="flex items-start gap-2">
                <div className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-(--accent-warning)/25 bg-(--bg-surface)/55">
                    <ShieldAlert className="h-3.5 w-3.5 text-(--accent-warning)" aria-hidden="true" />
                </div>
                <div className="min-w-0 flex-1 space-y-2">
                    <div className="flex flex-wrap items-center gap-1.5">
                        <span className="font-semibold text-(--fg-primary)">{t('toolCall.editImpact.title')}</span>
                        <CompactBadge label={t('toolCall.editImpact.riskBadge', { risk: risk || t('toolCall.badges.unknown') })} tone={risk} />
                        <CompactBadge label={t('toolCall.editImpact.confidenceBadge', { confidence: confidence || t('toolCall.badges.unknown') })} tone={confidence} />
                        <span className="ml-auto inline-flex items-center gap-1 text-[9px] font-semibold uppercase tracking-[0.14em] text-(--fg-tertiary)">
                            {statusIcon}
                            {statusText}
                        </span>
                    </div>
                    <div className="grid grid-cols-3 gap-1.5">
                        <div className="rounded border border-(--border-subtle) bg-(--bg-surface)/45 px-2 py-1">
                            <div className="text-[9px] uppercase tracking-[0.12em] text-(--fg-tertiary)">{t('toolCall.editImpact.files')}</div>
                            <div className="text-sm font-semibold text-(--fg-primary)">{impactedFiles.length}</div>
                        </div>
                        <div className="rounded border border-(--border-subtle) bg-(--bg-surface)/45 px-2 py-1">
                            <div className="text-[9px] uppercase tracking-[0.12em] text-(--fg-tertiary)">{t('toolCall.editImpact.tests')}</div>
                            <div className="text-sm font-semibold text-(--fg-primary)">{likelyTests.length}</div>
                        </div>
                        <div className="rounded border border-(--border-subtle) bg-(--bg-surface)/45 px-2 py-1">
                            <div className="text-[9px] uppercase tracking-[0.12em] text-(--fg-tertiary)">{t('toolCall.editImpact.refs')}</div>
                            <div className="text-sm font-semibold text-(--fg-primary)">{referenceCount}</div>
                        </div>
                    </div>
                    {impactedFiles.length > 0 && (
                        <div className="space-y-1">
                            {impactedFiles.slice(0, 4).map((file, index) => {
                                const path = asString(file.path);
                                const score = asNumber(file.score);
                                const reasons = asArray(file.reasons).map(asString).filter(Boolean);
                                const ranges = asArray(file.suggested_ranges);
                                if (!path) {
                                    return null;
                                }
                                return (
                                    <div key={`${path}-${index}`} className="space-y-0.5 rounded border border-(--border-subtle) bg-(--bg-app)/35 px-2 py-1">
                                        <div className="flex min-w-0 items-center gap-2">
                                            <PathButton path={path} onOpenFile={onOpenFile} />
                                            <div className="ml-auto flex shrink-0 items-center gap-1.5">
                                                {score !== null && <span className="text-[9px] text-(--fg-tertiary)">{score}</span>}
                                                <SuggestedRangeLabel ranges={ranges} />
                                            </div>
                                        </div>
                                        {reasons[0] && <div className="truncate text-[9px] text-(--fg-tertiary)">{reasons[0]}</div>}
                                    </div>
                                );
                            })}
                        </div>
                    )}
                    {likelyTests.length > 0 && (
                        <div className="flex flex-wrap gap-1.5">
                            {likelyTests.slice(0, 4).map((test, index) => {
                                const path = asString(test.path);
                                if (!path) {
                                    return null;
                                }
                                return (
                                    <span key={`${path}-${index}`} className="inline-flex max-w-full items-center gap-1 rounded-full border border-(--accent-mention)/20 bg-[color-mix(in_srgb,var(--accent-mention)_7%,transparent)] px-1.5 py-0.5">
                                        <GitBranch className="h-2.5 w-2.5 shrink-0 text-(--accent-mention)" aria-hidden="true" />
                                        <PathButton path={path} onOpenFile={onOpenFile}>{path.split(/[/\\]/).slice(-2).join('/')}</PathButton>
                                    </span>
                                );
                            })}
                        </div>
                    )}
                    {nextSteps[0] && (
                        <div className="rounded border border-(--accent-planning)/20 bg-[color-mix(in_srgb,var(--accent-planning)_7%,transparent)] px-2 py-1.5 text-[10px] leading-4 text-(--fg-secondary)">
                            {nextSteps[0]}
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
};

const ToolCallDisplayComponent: React.FC<ToolCallDisplayProps> = ({
    toolCall,
    status = 'pending',
    result,
    onStopCommand,
    onUndo,
    onOpenFile,
    workspaceRoot
}) => {
    const { t } = useTranslation();
    const [copied, setCopied] = useState(false);
    const [isExpanded, setIsExpanded] = useState(false);
    const [shouldFadeComplete, setShouldFadeComplete] = useState(status === 'complete');
    const previousStatusRef = useRef(status);
    const isRunCommand = toolCall.function.name === 'run_command';

    useEffect(() => {
        const previousStatus = previousStatusRef.current;
        previousStatusRef.current = status;

        if (status !== 'complete') {
            setShouldFadeComplete(false);
            return;
        }

        if (previousStatus === 'complete') {
            setShouldFadeComplete(true);
            return;
        }

        setShouldFadeComplete(false);
        const timerId = window.setTimeout(() => {
            setShouldFadeComplete(true);
        }, COMPLETE_VISUAL_HOLD_MS);

        return () => {
            window.clearTimeout(timerId);
        };
    }, [status]);

    const handleCopyCommand = useCallback(async (command: string) => {
        try {
            await navigator.clipboard.writeText(command);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        } catch (err) {
            console.error('Failed to copy command:', err);
        }
    }, []);

    const getStatusTone = () => {
        switch (status) {
            case 'executing':
                return 'var(--accent-ai)';
            case 'complete':
                return 'var(--accent-mention)';
            case 'error':
                return 'var(--state-danger)';
            case 'skipped':
                return 'var(--accent-warning)';
            default:
                return 'var(--accent-planning)';
        }
    };

    const getStatusIcon = () => {
        const style = { color: getStatusTone() };
        switch (status) {
            case 'executing':
                return <Loader2 aria-hidden="true" className="w-3.5 h-3.5 animate-spin" style={style} />;
            case 'complete':
                return <CheckCircle2 aria-hidden="true" className="w-3.5 h-3.5" style={style} />;
            case 'error':
                return <XCircle aria-hidden="true" className="w-3.5 h-3.5" style={style} />;
            case 'skipped':
                return <XCircle aria-hidden="true" className="w-3.5 h-3.5" style={style} />;
            default:
                return <Zap aria-hidden="true" className="w-3.5 h-3.5" style={style} />;
        }
    };

    const getStatusText = () => {
        switch (status) {
            case 'executing':
                return t('toolCall.status.executing');
            case 'complete':
                return t('toolCall.status.complete');
            case 'error':
                return t('toolCall.status.failed');
            case 'skipped':
                return t('toolCall.status.skipped');
            default:
                return t('toolCall.status.pending');
        }
    };

    const isComplete = status === 'complete';
    const isVisuallyComplete = isComplete && shouldFadeComplete;
    const canStopRunCommand = isRunCommand && onStopCommand && (status === 'executing' || status === 'pending');

    // Get friendly tool name
    const getFriendlyToolName = (name: string, args?: Record<string, unknown>): string => {
        // Special handling for apply_patch to show patch count
        if (name === 'apply_patch' && args) {
            const patches = args.patches as Array<unknown> | undefined;
            if (patches && patches.length > 1) {
                return t('toolCall.tools.applyPatchMany', {
                    count: patches.length,
                    defaultValue: `Applying ${patches.length} Code Changes`,
                });
            }
        }

        const nameMap: Record<string, { key: string; fallback: string }> = {
            'apply_patch': { key: 'toolCall.tools.applyPatch', fallback: 'Applying Code Changes' },
            'edit_file': { key: 'toolCall.tools.editFile', fallback: 'Editing File' },
            'read_file': { key: 'toolCall.tools.readFile', fallback: 'Reading File' },
            'write_file': { key: 'toolCall.tools.writeFile', fallback: 'Writing File' },
            'fast_context': { key: 'toolCall.tools.fastContext', fallback: 'Planning Context' },
            'edit_impact': { key: 'toolCall.tools.editImpact', fallback: 'Analyzing Edit Impact' },
            'symbol_search': { key: 'toolCall.tools.symbolSearch', fallback: 'Searching Symbols' },
            'semantic_anchor_search': { key: 'toolCall.tools.semanticAnchorSearch', fallback: 'Searching Semantic References' },
            'symbol_resolve': { key: 'toolCall.tools.symbolResolve', fallback: 'Resolving Symbol' },
            'symbol_related': { key: 'toolCall.tools.symbolRelated', fallback: 'Finding Related Symbols' },
            'symbol_outline': { key: 'toolCall.tools.symbolOutline', fallback: 'Reading Symbol Outline' },
            'symbol_references': { key: 'toolCall.tools.symbolReferences', fallback: 'Finding Symbol References' },
            'symbol_graph': { key: 'toolCall.tools.symbolGraph', fallback: 'Exploring Symbol Graph' },
            'symbol_trace': { key: 'toolCall.tools.symbolTrace', fallback: 'Tracing Symbol Relationships' },
            'symbol_path': { key: 'toolCall.tools.symbolPath', fallback: 'Finding Symbol Path' },
            'symbol_query': { key: 'toolCall.tools.symbolQuery', fallback: 'Querying Symbol Graph' },
            'symbol_architecture': { key: 'toolCall.tools.symbolArchitecture', fallback: 'Mapping Code Architecture' },
            'symbol_schema': { key: 'toolCall.tools.symbolSchema', fallback: 'Checking Symbol Index' },
            'list_files': { key: 'toolCall.tools.listFiles', fallback: 'Listing Files' },
            'grep_search': { key: 'toolCall.tools.grepSearch', fallback: 'Searching Code' },
            'run_command': { key: 'toolCall.tools.runCommand', fallback: 'Running Command' },
            'create_file': { key: 'toolCall.tools.createFile', fallback: 'Creating File' },
            'delete_file': { key: 'toolCall.tools.deleteFile', fallback: 'Deleting File' },
            'list_directory': { key: 'toolCall.tools.listDirectory', fallback: 'Listing Directory' },
            'get_workspace_structure': { key: 'toolCall.tools.getWorkspaceStructure', fallback: 'Analyzing Workspace' },
            'codebase_search': { key: 'toolCall.tools.codebaseSearch', fallback: 'Searching Codebase' },
            'get_editor_state': { key: 'toolCall.tools.getEditorState', fallback: 'Getting Editor State' },
            'read_file_range': { key: 'toolCall.tools.readFileRange', fallback: 'Reading File Range' },
            'find_files': { key: 'toolCall.tools.findFiles', fallback: 'Finding Files' },
            'find_files_glob': { key: 'toolCall.tools.findFilesGlob', fallback: 'Finding Files (Glob)' },
            'glob': { key: 'toolCall.tools.glob', fallback: 'Glob Search' },
            'find_by_name': { key: 'toolCall.tools.findByName', fallback: 'Find Files by Name' },
            'view_file_outline': { key: 'toolCall.tools.viewFileOutline', fallback: 'Viewing File Outline' },
            'search_web': { key: 'toolCall.tools.searchWeb', fallback: 'Searching Web' },
            'read_url_content': { key: 'toolCall.tools.readUrlContent', fallback: 'Reading URL' },
            'browser_subagent': { key: 'toolCall.tools.browserSubagent', fallback: 'Browser Agent' },
            'command_status': { key: 'toolCall.tools.commandStatus', fallback: 'Checking Command' },
            'send_command_input': { key: 'toolCall.tools.sendCommandInput', fallback: 'Sending Input' },
            'read_terminal': { key: 'toolCall.tools.readTerminal', fallback: 'Reading Terminal' },
            'list_dir': { key: 'toolCall.tools.listDir', fallback: 'Listing Directory' },
            'view_file': { key: 'toolCall.tools.viewFile', fallback: 'Viewing File' },
            'view_code_item': { key: 'toolCall.tools.viewCodeItem', fallback: 'Viewing Code Item' },
            'generate_image': { key: 'toolCall.tools.generateImage', fallback: 'Generating Image' },
            'multi_replace_file_content': { key: 'toolCall.tools.multiReplaceFileContent', fallback: 'Multi-Edit File' },
            'replace_file_content': { key: 'toolCall.tools.replaceFileContent', fallback: 'Replacing Content' },
            'write_to_file': { key: 'toolCall.tools.writeToFile', fallback: 'Writing to File' },
            'list_resources': { key: 'toolCall.tools.listResources', fallback: 'Listing Resources' },
            'read_resource': { key: 'toolCall.tools.readResource', fallback: 'Reading Resource' }
        };
        const mapped = nameMap[name];
        if (!mapped) return name;
        return t(mapped.key, { defaultValue: mapped.fallback });
    };

    // Parse arguments to display them nicely. Memoized: on long agentic runs
    // a message can hold hundreds of tool calls, and re-parsing every call's
    // arguments on every streaming re-render is a real CPU cost in WebKit.
    const parsedArgs = React.useMemo<Record<string, unknown>>(() => {
        try {
            return JSON.parse(toolCall.function.arguments);
        } catch {
            return { raw: toolCall.function.arguments };
        }
    }, [toolCall.function.arguments]);

    // For run_command, extract the command for display and copy
    const commandText = isRunCommand ? (parsedArgs.command as string || parsedArgs.CommandLine as string || '') : '';
    const cwdText = isRunCommand ? (parsedArgs.cwd as string || parsedArgs.Cwd as string || '') : '';
    const pathText = (
        parsedArgs.path as string
        || parsedArgs.Path as string
        || parsedArgs.file_path as string
        || parsedArgs.filePath as string
        || parsedArgs.filepath as string
        || parsedArgs.filename as string
        || parsedArgs.TargetFile as string
        || parsedArgs.target_file as string
        || parsedArgs.absolute_path as string
        || ''
    );

    const toLineNumber = (value: unknown): number | null => {
        if (typeof value === 'number' && Number.isFinite(value)) {
            return value;
        }
        if (typeof value === 'string') {
            const parsed = Number.parseInt(value, 10);
            return Number.isNaN(parsed) ? null : parsed;
        }
        return null;
    };
    const startLine = toLineNumber(
        parsedArgs.start_line
        ?? parsedArgs.startLine
        ?? parsedArgs.line_start
        ?? parsedArgs.lineStart
        ?? parsedArgs.from_line
        ?? parsedArgs.fromLine
    );
    const endLine = toLineNumber(
        parsedArgs.end_line
        ?? parsedArgs.endLine
        ?? parsedArgs.line_end
        ?? parsedArgs.lineEnd
        ?? parsedArgs.to_line
        ?? parsedArgs.toLine
    );
    const rangeSuffix = startLine !== null && endLine !== null
        ? `:${startLine}-${endLine}`
        : startLine !== null
            ? `:${startLine}`
            : '';

    // For search tools, extract the search query
    const searchQuery = (parsedArgs.pattern as string || parsedArgs.query as string || parsedArgs.regex as string || parsedArgs.Query as string || '');
    const getLastPathSegments = (value: string, count: number) => {
        const parts = value.split(/[/\\]/).filter(Boolean);
        return parts.slice(-count).join('/');
    };
    const relativePathText = toWorkspaceRelativePath(pathText, workspaceRoot);
    const baseDisplayPathText = toolCall.function.name === 'list_directory'
        ? getLastPathSegments(relativePathText, 2) || relativePathText
        : relativePathText;
    const displayPathText = toolCall.function.name === 'read_file_range' && baseDisplayPathText
        ? `${baseDisplayPathText}${rangeSuffix}`
        : baseDisplayPathText;
    const detailItems = [
        relativePathText ? { label: t('toolCall.details.path'), value: relativePathText } : null,
        searchQuery ? { label: t('toolCall.details.query'), value: searchQuery } : null,
        result ? { label: status === 'error' ? t('toolCall.details.error') : t('toolCall.details.result'), value: result } : null,
    ].filter((item): item is { label: string; value: string } => !!item);
    // Result payloads are up to 48 KiB of JSON; parse once per result change,
    // not on every render.
    const jsonResult = React.useMemo(() => parseJsonRecord(result), [result]);
    const resultCount = isComplete && isSymbolResultTool(toolCall.function.name)
        ? symbolResultCount(jsonResult)
        : null;

    if (toolCall.function.name === 'fast_context' && jsonResult) {
        return (
            <FastContextResultCard
                payload={jsonResult}
                statusText={getStatusText()}
                statusIcon={getStatusIcon()}
                isVisuallyComplete={isVisuallyComplete}
                onOpenFile={onOpenFile}
            />
        );
    }

    if (toolCall.function.name === 'edit_impact' && jsonResult) {
        return (
            <EditImpactResultCard
                payload={jsonResult}
                statusText={getStatusText()}
                statusIcon={getStatusIcon()}
                isVisuallyComplete={isVisuallyComplete}
                onOpenFile={onOpenFile}
            />
        );
    }

    // Compact inline display for most tools, expanded for run_command
    if (!isRunCommand) {
        return (
            <div
                className={`group/tool border-l-2 pl-2.5 text-[11px] transition-opacity duration-1000 ease-out ${isVisuallyComplete ? 'opacity-45' : 'opacity-100'}`}
                style={{ borderLeftColor: getStatusTone() }}
            >
                <div className="flex items-start gap-2 py-1">
                    <div className="flex h-5 w-5 shrink-0 items-center justify-center text-(--fg-tertiary)">
                        {getStatusIcon()}
                    </div>
                    <div className="min-w-0 flex-1 space-y-0.5">
                        <div className="flex items-start gap-2">
                            <div className="min-w-0 flex flex-1 items-center gap-2">
                                <span className={`shrink-0 text-[11px] font-medium ${isVisuallyComplete ? 'text-(--fg-tertiary)' : 'text-(--fg-primary)'}`}>
                                    {getFriendlyToolName(toolCall.function.name, parsedArgs)}
                                </span>
                                {displayPathText && (
                                    <button
                                        type="button"
                                        onClick={() => onOpenFile?.(pathText || displayPathText)}
                                        disabled={!onOpenFile}
                                        className={`min-w-0 flex-1 truncate text-left text-[10px] transition-colors ${onOpenFile
                                            ? isVisuallyComplete
                                                ? 'text-(--fg-tertiary) hover:text-(--fg-secondary)'
                                                : 'text-(--fg-secondary) hover:text-(--fg-primary)'
                                        : 'text-(--fg-tertiary)'
                                            }`}
                                        title={pathText || displayPathText}
                                        aria-label={pathText || displayPathText}
                                    >
                                        {displayPathText}
                                    </button>
                                )}
                                {!displayPathText && searchQuery && (
                                    <span
                                        className="min-w-0 flex-1 truncate text-[10px] font-mono text-(--fg-tertiary)"
                                        title={searchQuery}
                                    >
                                        {searchQuery}
                                    </span>
                                )}
                            </div>
                            <div className="ml-auto flex shrink-0 items-center gap-1 pl-2">
                                <span className={`text-[9px] font-semibold uppercase tracking-[0.14em] ${status === 'complete'
                                    ? 'text-(--accent-mention)'
                                    : status === 'executing'
                                        ? 'text-(--accent-ai)'
                                        : status === 'error'
                                            ? 'text-(--state-danger)'
                                            : status === 'skipped'
                                                ? 'text-(--accent-warning)'
                                                : 'text-(--accent-planning)'
                                    }`}>
                                    {getStatusText()}
                                </span>
                                {detailItems.length > 0 && (
                                    <button
                                        type="button"
                                        onClick={() => setIsExpanded(!isExpanded)}
                                        aria-expanded={isExpanded}
                                        aria-label={isExpanded ? t('toolCall.hideDetails') : t('toolCall.showDetails')}
                                        className="rounded p-0.5 text-(--fg-tertiary) transition-colors hover:text-(--fg-primary)"
                                        title={isExpanded ? t('toolCall.hideDetails') : t('toolCall.showDetails')}
                                    >
                                        {isExpanded ? (
                                            <ChevronDown aria-hidden="true" className="w-3 h-3" />
                                        ) : (
                                            <ChevronRight aria-hidden="true" className="w-3 h-3" />
                                        )}
                                    </button>
                                )}
                            </div>
                        </div>
                        <div className="flex flex-wrap items-center gap-1.5 pl-0.5">
                            {searchQuery && displayPathText && (
                                <span
                                    className="min-w-0 max-w-full truncate text-[10px] font-mono text-(--fg-tertiary)"
                                    title={searchQuery}
                                >
                                    {searchQuery}
                                </span>
                            )}
                            {resultCount !== null && (
                                <span className="shrink-0 text-[10px] text-(--fg-tertiary)">
                                    {t('toolCall.resultCount', { count: resultCount })}
                                </span>
                            )}
                            {onUndo && status === 'complete' && (
                                <button
                                    type="button"
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        onUndo();
                                    }}
                                    className="flex items-center gap-1 rounded px-1 py-0.5 text-[9px] text-(--fg-tertiary) transition-colors hover:text-(--state-danger)"
                                    title={t('toolCall.undoChanges')}
                                >
                                    <RotateCcw aria-hidden="true" className="w-2.5 h-2.5" />
                                    {t('toolCall.undo')}
                                </button>
                            )}
                        </div>
                    </div>
                </div>
                {isExpanded && detailItems.length > 0 && (
                    <div
                        className="ml-7 border-l pl-3 pb-1 pt-1"
                        style={{ borderLeftColor: 'color-mix(in srgb, var(--border-default) 82%, transparent)' }}
                    >
                        <div className="space-y-1.5">
                            {detailItems.map((item) => (
                                <div key={item.label} className="space-y-0.5">
                                    <div className="text-[9px] font-semibold uppercase tracking-[0.16em] text-(--fg-tertiary)">
                                        {item.label}
                                    </div>
                                    <div className="wrap-break-word text-[11px] leading-4 text-(--fg-primary)">
                                        {item.value}
                                    </div>
                                </div>
                            ))}
                        </div>
                    </div>
                )}
            </div>
        );
    }

    return (
        <div
            data-tool-call-id={toolCall.id}
            className={`border-l-2 pl-2.5 transition-opacity duration-1000 ease-out ${isVisuallyComplete ? 'opacity-45' : 'opacity-100'}`}
            style={{ borderLeftColor: getStatusTone() }}
        >
            <div className="flex items-center justify-between gap-2 py-1">
                <div className="flex min-w-0 items-center gap-2">
                    <div className="flex h-5 w-5 items-center justify-center text-(--fg-tertiary)">
                        {getStatusIcon()}
                    </div>
                    <div className="min-w-0">
                        <span className={`block truncate text-[11px] font-medium ${isVisuallyComplete ? 'text-(--fg-tertiary)' : 'text-(--fg-primary)'}`}>
                            {getFriendlyToolName(toolCall.function.name, parsedArgs)}
                        </span>
                    </div>
                </div>
                <div className="flex items-center gap-1.5">
                    {canStopRunCommand && (
                        <button
                            type="button"
                            onClick={(e) => {
                                e.stopPropagation();
                                onStopCommand();
                            }}
                            className="inline-flex h-6 items-center gap-1 rounded-[calc(var(--panel-radius)*0.35)] border border-(--state-danger)/45 px-1.5 text-[9px] font-semibold uppercase tracking-[0.12em] text-(--state-danger) transition-colors hover:bg-(--state-danger)/10"
                            title={t('toolCall.stopCommand')}
                            aria-label={t('toolCall.stopCommand')}
                        >
                            <StopCircle aria-hidden="true" className="h-3 w-3" />
                            {t('common.stop', { defaultValue: 'Stop' })}
                        </button>
                    )}
                    <span className={`text-[9px] font-semibold uppercase tracking-[0.14em] ${status === 'complete' ? 'text-(--accent-mention)' :
                        status === 'executing' ? 'text-(--accent-ai)' :
                            status === 'error' ? 'text-(--state-danger)' :
                                status === 'skipped' ? 'text-(--accent-warning)' : 'text-(--accent-planning)'
                        }`}>
                        {getStatusText()}
                    </span>
                </div>
            </div>

            {commandText && (
                <div
                    className="ml-7 border-l pl-3 pb-1 pt-1"
                    style={{ borderLeftColor: 'color-mix(in srgb, var(--border-default) 82%, transparent)' }}
                >
                    <div className="flex items-start gap-2">
                        <code className={`flex-1 break-all text-[12px] font-mono leading-5 select-text ${isVisuallyComplete ? 'text-(--fg-tertiary)' : 'text-(--fg-primary)'}`}>
                            {cwdText && <span className="text-(--fg-tertiary)">{cwdText}$ </span>}
                            {commandText}
                        </code>
                        <button
                            type="button"
                            onClick={() => handleCopyCommand(commandText)}
                            className="group/copy shrink-0 rounded p-1 text-(--fg-tertiary) transition-colors hover:text-(--fg-primary)"
                            title={t('toolCall.copyCommand')}
                            aria-label={t('toolCall.copyCommand')}
                        >
                            {copied ? (
                                <Check aria-hidden="true" className="w-3.5 h-3.5 text-(--accent-mention)" />
                            ) : (
                                <Copy aria-hidden="true" className="w-3.5 h-3.5 text-(--fg-tertiary) group-hover/copy:text-(--fg-primary)" />
                            )}
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
};

// Memoized: during long agentic runs the active assistant message accumulates
// hundreds of tool calls, and every streaming chunk re-renders the whole
// message. Unchanged ToolCall objects keep referential identity through
// useChatV2 updates, so this bail-out makes a chunk update O(changed tools)
// instead of O(all tools). Callbacks are compared by presence, not identity —
// the closures are recreated per parent render but their behavior is stable.
export const ToolCallDisplay = React.memo(ToolCallDisplayComponent, (prevProps, nextProps) => {
    if (prevProps.status !== nextProps.status) return false;
    if (prevProps.result !== nextProps.result) return false;
    if (prevProps.workspaceRoot !== nextProps.workspaceRoot) return false;
    const prevTool = prevProps.toolCall;
    const nextTool = nextProps.toolCall;
    if (prevTool !== nextTool) {
        if (prevTool.id !== nextTool.id) return false;
        if (prevTool.function.name !== nextTool.function.name) return false;
        if (prevTool.function.arguments !== nextTool.function.arguments) return false;
        if (prevTool.status !== nextTool.status) return false;
        if (prevTool.result !== nextTool.result) return false;
    }
    if (!!prevProps.onStopCommand !== !!nextProps.onStopCommand) return false;
    if (!!prevProps.onUndo !== !!nextProps.onUndo) return false;
    if (!!prevProps.onOpenFile !== !!nextProps.onOpenFile) return false;
    return true;
});
