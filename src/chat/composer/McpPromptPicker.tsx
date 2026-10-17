import React, { useEffect, useId, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { MessageSquareText, X } from 'lucide-react';
import type { McpPromptEntry, McpPromptListing, McpPromptRender, McpPromptServer } from '../../types/integrations';
import { integrationProbeErrorKey } from '../../utils/integrationProbe';
import { insertionFromPrompt, isEmptyInsertion, type PromptInsertion } from '../../utils/mcpPrompts';

const PREVIEW_CHARS = 4000;
const rowClass = 'flex w-full items-start gap-2 rounded-(--radius-control) px-2 py-1.5 text-left text-xs text-(--fg-secondary) hover:bg-(--row-hover) hover:text-(--fg-primary)';
const buttonClass = 'px-3 py-1.5 text-xs font-medium rounded-(--radius-control) transition-all duration-(--transition-fast)';
const clipped = (text: string) => text.length > PREVIEW_CHARS ? `${text.slice(0, PREVIEW_CHARS)}…` : text;

/** A prompt is generated only for the entry and arguments the user picks, and
 * nothing reaches the composer until the user confirms the preview. */
export const McpPromptPicker: React.FC<{
    onInsert: (insertion: PromptInsertion) => void;
    onClose: () => void;
}> = ({ onInsert, onClose }) => {
    const { t } = useTranslation();
    const titleId = useId();
    const generation = useRef(0);
    const [servers, setServers] = useState<McpPromptServer[] | null>(null);
    const [server, setServer] = useState<McpPromptServer | null>(null);
    const [listing, setListing] = useState<McpPromptListing | null>(null);
    const [filter, setFilter] = useState('');
    const [prompt, setPrompt] = useState<McpPromptEntry | null>(null);
    const [values, setValues] = useState<Record<string, string>>({});
    const [render, setRender] = useState<McpPromptRender | null>(null);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const fail = (failure: unknown) => setError(integrationProbeErrorKey(failure));
    useEffect(() => {
        const current = ++generation.current;
        invoke<McpPromptServer[]>('get_mcp_prompt_servers').then(next => {
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
        setListing(null); setPrompt(null); setRender(null); setError(null);
        if (!server) return;
        const current = ++generation.current;
        setBusy(true);
        invoke<McpPromptListing>('list_mcp_prompts', { connectionId: server.connection_id })
            .then(next => { if (current === generation.current) setListing(next); })
            .catch(failure => { if (current === generation.current) fail(failure); })
            .finally(() => { if (current === generation.current) setBusy(false); });
    }, [server]);

    const needle = filter.trim().toLowerCase();
    // At most 1000 entries (backend bound), so filtering per render is cheap.
    const prompts = listing?.prompts.filter(item => !needle || [item.name, item.title, item.description].some(field => field?.toLowerCase().includes(needle))) ?? [];

    const generate = async (entry: McpPromptEntry, args: Record<string, string>) => {
        if (!server) return;
        const current = ++generation.current;
        setBusy(true); setError(null); setRender(null);
        try {
            // Empty optional arguments are omitted rather than sent as "".
            const sent = Object.fromEntries(Object.entries(args).filter(([, value]) => value !== ''));
            const result = await invoke<McpPromptRender>('get_mcp_prompt', { connectionId: server.connection_id, name: entry.name, arguments: sent });
            if (current === generation.current) setRender(result);
        } catch (failure) {
            if (current === generation.current) fail(failure);
        } finally {
            if (current === generation.current) setBusy(false);
        }
    };
    const choose = (entry: McpPromptEntry) => {
        setPrompt(entry); setRender(null); setError(null);
        const initial = Object.fromEntries(entry.arguments.map(argument => [argument.name, '']));
        setValues(initial);
        if (entry.arguments.length === 0) void generate(entry, initial);
    };
    const missing = prompt?.arguments.some(argument => argument.required && !values[argument.name]?.trim()) ?? true;
    const insertion = render && server
        ? insertionFromPrompt(server.name, render, { user: t('chat.mcpPrompts.roleUser'), assistant: t('chat.mcpPrompts.roleAssistant') })
        : null;
    const empty = insertion ? isEmptyInsertion(insertion) : false;

    return (
        <div className="fixed inset-0 z-9999 flex items-center justify-center">
            <div aria-hidden="true" className="absolute inset-0 bg-black/70" onClick={onClose} />
            <div role="dialog" aria-modal="true" aria-labelledby={titleId} className="relative mx-4 flex max-h-[80vh] w-full max-w-2xl flex-col overflow-hidden rounded-(--radius-dialog) border border-(--focus-ring) bg-(--surface-overlay) shadow-(--shadow-dialog)">
                <div className="flex items-center justify-between border-b border-(--separator-subtle) px-4 py-3">
                    <h2 id={titleId} className="text-sm font-semibold text-(--fg-primary)">{t('chat.mcpPrompts.title')}</h2>
                    <button type="button" aria-label={t('common.close')} onClick={onClose} className="rounded-(--radius-control) p-1 text-(--fg-tertiary) hover:bg-(--row-hover) hover:text-(--fg-primary)">
                        <X className="h-4 w-4" aria-hidden="true" />
                    </button>
                </div>
                <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4 text-sm text-(--fg-secondary)">
                    <p className="text-xs text-(--fg-tertiary)">{t('chat.mcpPrompts.help')}</p>
                    {servers === null ? <p role="status">{t('chat.mcpPrompts.loading')}</p> : null}
                    {servers?.length === 0 ? <p role="status">{t('chat.mcpPrompts.noServers')}</p> : null}
                    {servers && servers.length > 1 ? (
                        <label className="flex flex-col gap-1 text-xs">
                            {t('chat.mcpPrompts.server')}
                            <select className="rounded-md border border-(--border-default) bg-(--bg-app) px-2 py-1.5 text-sm" value={server?.connection_id ?? ''} onChange={event => setServer(servers.find(item => item.connection_id === event.target.value) ?? null)}>
                                <option value="" disabled>{t('chat.mcpPrompts.chooseServer')}</option>
                                {servers.map(item => <option key={item.connection_id} value={item.connection_id}>{item.name}</option>)}
                            </select>
                        </label>
                    ) : null}
                    {listing ? (
                        <>
                            <input type="search" value={filter} onChange={event => setFilter(event.target.value)} placeholder={t('chat.mcpPrompts.filter')} aria-label={t('chat.mcpPrompts.filter')} className="rounded-md border border-(--border-default) bg-(--bg-app) px-2 py-1.5 text-sm" />
                            {listing.prompts.length === 0 ? <p role="status">{t('chat.mcpPrompts.empty')}</p> : null}
                            {listing.truncated ? <p className="text-xs text-(--state-warning)">{t('chat.mcpPrompts.listTruncated')}</p> : null}
                            <ul className="flex max-h-56 flex-col overflow-y-auto" aria-label={t('chat.mcpPrompts.title')}>
                                {prompts.map(item => (
                                    <li key={item.name}>
                                        <button type="button" className={rowClass} aria-pressed={prompt?.name === item.name} disabled={busy} onClick={() => choose(item)}>
                                            <MessageSquareText className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                                            <span className="min-w-0">
                                                <span className="block truncate font-medium">{item.title || item.name}</span>
                                                {item.description ? <span className="block truncate text-[10px] text-(--fg-tertiary)">{item.description}</span> : null}
                                            </span>
                                        </button>
                                    </li>
                                ))}
                            </ul>
                        </>
                    ) : null}
                    {prompt && prompt.arguments.length > 0 ? (
                        <form className="flex flex-col gap-2 rounded-md border border-(--border-subtle) p-3" onSubmit={event => { event.preventDefault(); if (!missing) void generate(prompt, values); }}>
                            {prompt.description ? <p className="text-xs text-(--fg-tertiary)">{prompt.description}</p> : null}
                            {prompt.arguments.map(argument => (
                                <label key={argument.name} className="flex flex-col gap-1 text-xs">
                                    <span>
                                        <code>{argument.title || argument.name}</code>
                                        {argument.required ? <span className="text-(--state-danger)"> *</span> : <span className="text-(--fg-tertiary)"> {t('chat.mcpPrompts.optional')}</span>}
                                    </span>
                                    {argument.description ? <span className="text-[10px] text-(--fg-tertiary)">{argument.description}</span> : null}
                                    <input required={argument.required} value={values[argument.name] ?? ''} onChange={event => setValues(current => ({ ...current, [argument.name]: event.target.value }))} className="rounded-md border border-(--border-default) bg-(--bg-app) px-2 py-1.5 text-sm" />
                                </label>
                            ))}
                            <button type="submit" disabled={busy || missing} className={`${buttonClass} self-end border border-(--border-default) text-(--fg-secondary) hover:bg-(--row-hover) disabled:opacity-50`}>{t('chat.mcpPrompts.generate')}</button>
                        </form>
                    ) : null}
                    {busy ? <p role="status">{t('chat.mcpPrompts.loading')}</p> : null}
                    {error ? <p role="alert" className="text-xs text-(--state-danger)">{t(error)}</p> : null}
                    {render && insertion ? (
                        <div className="flex flex-col gap-2 rounded-md border border-(--border-subtle) p-3">
                            <span className="text-xs font-semibold text-(--fg-primary)">{t('chat.mcpPrompts.preview')}</span>
                            {render.description ? <p className="text-xs text-(--fg-tertiary)">{render.description}</p> : null}
                            <ol className="flex max-h-64 flex-col gap-2 overflow-y-auto">
                                {render.messages.map((message, index) => (
                                    <li key={index} className="flex flex-col gap-1">
                                        <span className="text-[10px] font-semibold uppercase tracking-wide text-(--fg-tertiary)">
                                            {message.role === 'user' ? t('chat.mcpPrompts.roleUser') : t('chat.mcpPrompts.roleAssistant')}
                                        </span>
                                        {message.part.kind === 'text' ? <pre className="whitespace-pre-wrap break-words rounded bg-(--bg-app) p-2 text-[11px]">{clipped(message.part.text)}</pre> : null}
                                        {message.part.kind === 'resource' ? <span className="text-xs">{t('chat.mcpPrompts.partResource', { uri: message.part.uri })}</span> : null}
                                        {message.part.kind === 'image' ? <span className="text-xs">{t('chat.mcpPrompts.partImage')}</span> : null}
                                        {message.part.kind === 'link' ? <span className="text-xs text-(--fg-tertiary)">{t('chat.mcpPrompts.partLink', { uri: message.part.uri })}</span> : null}
                                        {message.part.kind === 'unsupported' ? <span className="text-xs text-(--state-warning)">{t('chat.mcpPrompts.partUnsupported')}</span> : null}
                                    </li>
                                ))}
                            </ol>
                            {render.messages.some(message => message.role === 'assistant') ? <p className="text-xs text-(--fg-tertiary)">{t('chat.mcpPrompts.mixedRoles')}</p> : null}
                            {render.truncated ? <p className="text-xs text-(--state-warning)">{t('chat.mcpPrompts.truncated')}</p> : null}
                            {insertion.links > 0 ? <p className="text-xs text-(--fg-tertiary)">{t('chat.mcpPrompts.links', { count: insertion.links })}</p> : null}
                            {insertion.unsupported > 0 ? <p className="text-xs text-(--state-warning)">{t('chat.mcpPrompts.unsupported', { count: insertion.unsupported })}</p> : null}
                            {empty ? <p className="text-xs">{t('chat.mcpPrompts.nothing')}</p> : null}
                        </div>
                    ) : null}
                </div>
                <div className="flex items-center justify-end gap-2 border-t border-(--separator-subtle) px-4 py-3">
                    <button type="button" onClick={onClose} className={`${buttonClass} text-(--fg-secondary) hover:bg-(--row-hover) hover:text-(--fg-primary)`}>{t('common.cancel')}</button>
                    <button type="button" disabled={!insertion || empty || busy} onClick={() => insertion && onInsert(insertion)} className={`${buttonClass} bg-(--accent-ai) text-(--fg-bright) hover:brightness-110 disabled:opacity-50`}>{t('chat.mcpPrompts.insert')}</button>
                </div>
            </div>
        </div>
    );
};
