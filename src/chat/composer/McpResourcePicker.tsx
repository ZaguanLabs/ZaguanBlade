import React, { useEffect, useId, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { FileText, LayoutTemplate, X } from 'lucide-react';
import type { McpResourceListing, McpResourceRead, McpResourceServer, McpResourceTemplateEntry } from '../../types/integrations';
import { integrationProbeErrorKey } from '../../utils/integrationProbe';
import { selectionFromRead, type ResourceSelection } from '../../utils/mcpResources';

type Source = { kind: 'resource'; uri: string; name: string } | { kind: 'template'; template: McpResourceTemplateEntry };
const PREVIEW_CHARS = 4000;
const rowClass = 'flex w-full items-start gap-2 rounded-(--radius-control) px-2 py-1.5 text-left text-xs text-(--fg-secondary) hover:bg-(--row-hover) hover:text-(--fg-primary)';
const buttonClass = 'px-3 py-1.5 text-xs font-medium rounded-(--radius-control) transition-all duration-(--transition-fast)';

/** Reads happen only for an entry the user picks; nothing is attached until
 * the user confirms the preview. */
export const McpResourcePicker: React.FC<{
    onAttach: (selection: ResourceSelection) => void;
    onClose: () => void;
}> = ({ onAttach, onClose }) => {
    const { t } = useTranslation();
    const titleId = useId();
    const generation = useRef(0);
    const [servers, setServers] = useState<McpResourceServer[] | null>(null);
    const [server, setServer] = useState<McpResourceServer | null>(null);
    const [listing, setListing] = useState<McpResourceListing | null>(null);
    const [filter, setFilter] = useState('');
    const [source, setSource] = useState<Source | null>(null);
    const [values, setValues] = useState<Record<string, string>>({});
    const [selection, setSelection] = useState<ResourceSelection | null>(null);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const fail = (failure: unknown) => setError(integrationProbeErrorKey(failure));
    useEffect(() => {
        const current = ++generation.current;
        invoke<McpResourceServer[]>('get_mcp_resource_servers').then(next => {
            if (current !== generation.current) return;
            setServers(next);
            if (next.length === 1) setServer(next[0]);
        }).catch(failure => { if (current === generation.current) { setServers([]); fail(failure); } });
        return () => { generation.current += 1; };
    }, []);
    useEffect(() => {
        const handleKeyDown = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
        document.addEventListener('keydown', handleKeyDown);
        return () => document.removeEventListener('keydown', handleKeyDown);
    }, [onClose]);
    useEffect(() => {
        setListing(null); setSource(null); setSelection(null); setError(null);
        if (!server) return;
        const current = ++generation.current;
        setBusy(true);
        invoke<McpResourceListing>('list_mcp_resources', { connectionId: server.connection_id })
            .then(next => { if (current === generation.current) setListing(next); })
            .catch(failure => { if (current === generation.current) fail(failure); })
            .finally(() => { if (current === generation.current) setBusy(false); });
    }, [server]);

    const needle = filter.trim().toLowerCase();
    const matches = (...fields: (string | null)[]) => !needle || fields.some(field => field?.toLowerCase().includes(needle));
    // At most 1000 entries (backend bound), so filtering per render is cheap.
    const resources = listing?.resources.filter(item => matches(item.name, item.title, item.uri, item.description)) ?? [];
    const templates = listing?.templates.filter(item => matches(item.name, item.title, item.uri_template, item.description)) ?? [];

    const read = async (next: Source) => {
        if (!server) return;
        setSource(next); setSelection(null); setError(null);
        if (next.kind === 'template') {
            setValues(Object.fromEntries(next.template.variables.map(name => [name, ''])));
            return;
        }
        await load(next, { uri: next.uri });
    };
    const load = async (next: Source, args: Record<string, unknown>) => {
        if (!server) return;
        const current = ++generation.current;
        setBusy(true); setError(null);
        try {
            const result = await invoke<McpResourceRead>('read_mcp_resource', { connectionId: server.connection_id, ...args });
            if (current !== generation.current) return;
            const name = next.kind === 'resource' ? next.name : next.template.name;
            setSelection(selectionFromRead(server.name, name, result));
        } catch (failure) {
            if (current === generation.current) fail(failure);
        } finally {
            if (current === generation.current) setBusy(false);
        }
    };
    const readTemplate = () => {
        if (source?.kind !== 'template') return;
        void load(source, { template: source.template.uri_template, values });
    };
    const empty = selection && !selection.attachment && selection.images.length === 0;

    return (
        <div className="fixed inset-0 z-9999 flex items-center justify-center">
            <div aria-hidden="true" className="absolute inset-0 bg-black/70" onClick={onClose} />
            <div role="dialog" aria-modal="true" aria-labelledby={titleId} className="relative mx-4 flex max-h-[80vh] w-full max-w-2xl flex-col overflow-hidden rounded-(--radius-dialog) border border-(--focus-ring) bg-(--surface-overlay) shadow-(--shadow-dialog)">
                <div className="flex items-center justify-between border-b border-(--separator-subtle) px-4 py-3">
                    <h2 id={titleId} className="text-sm font-semibold text-(--fg-primary)">{t('chat.mcpResources.title')}</h2>
                    <button type="button" aria-label={t('common.close')} onClick={onClose} className="rounded-(--radius-control) p-1 text-(--fg-tertiary) hover:bg-(--row-hover) hover:text-(--fg-primary)">
                        <X className="h-4 w-4" aria-hidden="true" />
                    </button>
                </div>
                <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4 text-sm text-(--fg-secondary)">
                    <p className="text-xs text-(--fg-tertiary)">{t('chat.mcpResources.help')}</p>
                    {servers === null ? <p role="status">{t('chat.mcpResources.loading')}</p> : null}
                    {servers?.length === 0 ? <p role="status">{t('chat.mcpResources.noServers')}</p> : null}
                    {servers && servers.length > 1 ? (
                        <label className="flex flex-col gap-1 text-xs">
                            {t('chat.mcpResources.server')}
                            <select className="rounded-md border border-(--border-default) bg-(--bg-app) px-2 py-1.5 text-sm" value={server?.connection_id ?? ''} onChange={event => setServer(servers.find(item => item.connection_id === event.target.value) ?? null)}>
                                <option value="" disabled>{t('chat.mcpResources.chooseServer')}</option>
                                {servers.map(item => <option key={item.connection_id} value={item.connection_id}>{item.name}</option>)}
                            </select>
                        </label>
                    ) : null}
                    {listing ? (
                        <>
                            <input type="search" value={filter} onChange={event => setFilter(event.target.value)} placeholder={t('chat.mcpResources.filter')} aria-label={t('chat.mcpResources.filter')} className="rounded-md border border-(--border-default) bg-(--bg-app) px-2 py-1.5 text-sm" />
                            {listing.resources.length === 0 && listing.templates.length === 0 ? <p role="status">{t('chat.mcpResources.empty')}</p> : null}
                            {listing.truncated ? <p className="text-xs text-(--state-warning)">{t('chat.mcpResources.listTruncated')}</p> : null}
                            <ul className="flex max-h-56 flex-col overflow-y-auto" aria-label={t('chat.mcpResources.title')}>
                                {resources.map(item => (
                                    <li key={`r:${item.uri}`}>
                                        <button type="button" className={rowClass} aria-pressed={source?.kind === 'resource' && source.uri === item.uri} disabled={busy} onClick={() => void read({ kind: 'resource', uri: item.uri, name: item.title || item.name })}>
                                            <FileText className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                                            <span className="min-w-0">
                                                <span className="block truncate font-medium">{item.title || item.name}</span>
                                                <span className="block truncate text-[10px] text-(--fg-tertiary)">{item.uri}{item.mime_type ? ` · ${item.mime_type}` : ''}</span>
                                            </span>
                                        </button>
                                    </li>
                                ))}
                                {templates.map(item => (
                                    <li key={`t:${item.uri_template}`}>
                                        <button type="button" className={rowClass} aria-pressed={source?.kind === 'template' && source.template.uri_template === item.uri_template} disabled={busy} onClick={() => void read({ kind: 'template', template: item })}>
                                            <LayoutTemplate className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                                            <span className="min-w-0">
                                                <span className="block truncate font-medium">{item.title || item.name}</span>
                                                <span className="block truncate text-[10px] text-(--fg-tertiary)">{t('chat.mcpResources.template')} · {item.uri_template}</span>
                                            </span>
                                        </button>
                                    </li>
                                ))}
                            </ul>
                        </>
                    ) : null}
                    {source?.kind === 'template' ? (
                        <form className="flex flex-col gap-2 rounded-md border border-(--border-subtle) p-3" onSubmit={event => { event.preventDefault(); readTemplate(); }}>
                            {source.template.description ? <p className="text-xs text-(--fg-tertiary)">{source.template.description}</p> : null}
                            {source.template.variables.map(name => (
                                <label key={name} className="flex flex-col gap-1 text-xs">
                                    <code>{name}</code>
                                    <input value={values[name] ?? ''} onChange={event => setValues(current => ({ ...current, [name]: event.target.value }))} className="rounded-md border border-(--border-default) bg-(--bg-app) px-2 py-1.5 text-sm" />
                                </label>
                            ))}
                            <button type="submit" disabled={busy} className={`${buttonClass} self-end border border-(--border-default) text-(--fg-secondary) hover:bg-(--row-hover)`}>{t('chat.mcpResources.read')}</button>
                        </form>
                    ) : null}
                    {busy ? <p role="status">{t('chat.mcpResources.loading')}</p> : null}
                    {error ? <p role="alert" className="text-xs text-(--state-danger)">{t(error)}</p> : null}
                    {selection ? (
                        <div className="flex flex-col gap-2 rounded-md border border-(--border-subtle) p-3">
                            <span className="text-xs font-semibold text-(--fg-primary)">{t('chat.mcpResources.preview')}</span>
                            {selection.attachment ? (
                                <>
                                    <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words rounded bg-(--bg-app) p-2 text-[11px]">{selection.attachment.text.slice(0, PREVIEW_CHARS)}{selection.attachment.text.length > PREVIEW_CHARS ? '…' : ''}</pre>
                                    <span className="text-[10px] text-(--fg-tertiary)">{t('chat.mcpResources.size', { kb: Math.max(1, Math.round(selection.attachment.bytes / 1024)) })}</span>
                                </>
                            ) : null}
                            {selection.attachment?.truncated ? <p className="text-xs text-(--state-warning)">{t('chat.mcpResources.truncated')}</p> : null}
                            {selection.images.length > 0 ? <p className="text-xs">{t('chat.mcpResources.images', { count: selection.images.length })}</p> : null}
                            {selection.unsupported > 0 ? <p className="text-xs text-(--state-warning)">{t('chat.mcpResources.unsupported', { count: selection.unsupported })}</p> : null}
                            {empty ? <p className="text-xs">{t('chat.mcpResources.nothing')}</p> : null}
                        </div>
                    ) : null}
                </div>
                <div className="flex items-center justify-end gap-2 border-t border-(--separator-subtle) px-4 py-3">
                    <button type="button" onClick={onClose} className={`${buttonClass} text-(--fg-secondary) hover:bg-(--row-hover) hover:text-(--fg-primary)`}>{t('common.cancel')}</button>
                    <button type="button" disabled={!selection || !!empty || busy} onClick={() => selection && onAttach(selection)} className={`${buttonClass} bg-(--accent-ai) text-(--fg-bright) hover:brightness-110 disabled:opacity-50`}>{t('chat.mcpResources.attach')}</button>
                </div>
            </div>
        </div>
    );
};
