import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AcpConfigChange, AcpPermission, AcpTerminal, AcpToolCall, AcpToolContent } from '../types/acp';
import { runningTurn, type AgentChat, type AgentTurn } from '../utils/acpConversation';
import { integrationProbeErrorKey } from '../utils/integrationProbe';
import { MarkdownRenderer } from './MarkdownRenderer';
import { AcpForwarding } from './AcpForwarding';
import { AcpConfigControls } from './AcpConfigControls';
import { stripAllAnsi } from './CommandOutputDisplay';

const buttonClass = 'rounded border border-(--border-default) px-3 py-1.5 text-xs hover:bg-(--bg-surface-hover) disabled:opacity-50';

type Terminals = Record<string, AcpTerminal>;

/** A command the agent ran in a terminal Blade owns, with its latest output. */
function TerminalCard({ terminal }: { terminal: AcpTerminal }) {
    const { t } = useTranslation();
    const { exit } = terminal;
    const failed = Boolean(exit && (exit.signal || exit.exit_code !== 0));
    const status = !exit ? t('chat.agent.terminal.running')
        : exit.signal ? t('chat.agent.terminal.signal', { signal: exit.signal })
            : t('chat.agent.terminal.exited', { code: exit.exit_code ?? '?' });
    const output = stripAllAnsi(terminal.output);
    return <div className="mt-1 rounded border border-(--border-subtle)">
        <div className="flex items-start justify-between gap-2 border-b border-(--border-subtle) px-2 py-1">
            <code className="min-w-0 break-all text-(--fg-primary)" title={terminal.cwd}>$ {terminal.command}</code>
            <span className={`shrink-0 ${failed ? 'text-(--state-danger)' : 'text-(--fg-tertiary)'}`}>{status}</span>
        </div>
        {terminal.truncated ? <p className="px-2 pt-1 text-(--fg-tertiary)">{t('chat.agent.terminal.truncated')}</p> : null}
        {output ? <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words px-2 py-1 font-mono">{output}</pre> : null}
    </div>;
}

function ToolContent({ item, terminals }: { item: AcpToolContent; terminals: Terminals }) {
    const { t } = useTranslation();
    if (item.type === 'diff') return <details className="mt-1">
        <summary className="cursor-pointer break-all">{t('chat.agent.tool.diff', { path: item.path })}</summary>
        {item.old_text !== null ? <pre className="my-1 max-h-48 overflow-auto whitespace-pre-wrap break-all bg-[color-mix(in_srgb,var(--state-danger)_8%,transparent)] p-2">{item.old_text}</pre> : null}
        <pre className="my-1 max-h-48 overflow-auto whitespace-pre-wrap break-all bg-[color-mix(in_srgb,var(--accent-green)_8%,transparent)] p-2">{item.new_text}</pre>
    </details>;
    if (item.type === 'terminal') {
        const terminal = terminals[item.terminal_id];
        return terminal ? <TerminalCard terminal={terminal} /> : <p className="mt-1">{t('chat.agent.tool.terminal', { id: item.terminal_id })}</p>;
    }
    const content = item.content;
    if (content.type === 'text') return <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap break-words">{content.text}</pre>;
    if (content.type === 'resource') return <p className="mt-1 break-all">{t('chat.agent.tool.resource', { uri: content.uri })}</p>;
    if (content.type === 'link') return <p className="mt-1 break-all">{content.name} ({content.uri})</p>;
    return <p className="mt-1">{t('chat.agent.tool.other')}</p>;
}

/** A tool call the agent reported. Display only: Blade never runs it. */
function ToolCallCard({ call, terminals, onOpenFile }: { call: AcpToolCall; terminals: Terminals; onOpenFile: (path: string) => void }) {
    const { t } = useTranslation();
    return <div className="rounded border border-(--border-subtle) px-2 py-1.5">
        <div className="flex items-start justify-between gap-2">
            <span className="min-w-0 break-words text-(--fg-primary)">{call.title || call.id}</span>
            <span className={`shrink-0 ${call.status === 'failed' ? 'text-(--state-danger)' : 'text-(--fg-tertiary)'}`}>{t(`chat.agent.tool.status.${call.status}`)}</span>
        </div>
        {call.locations.map((location, index) => <button key={index} type="button" className="block max-w-full truncate text-left text-(--accent-mention) hover:underline"
            onClick={() => onOpenFile(location.path)}>{location.path}{location.line !== null ? `:${location.line}` : ''}</button>)}
        {call.content.map((item, index) => <ToolContent key={index} item={item} terminals={terminals} />)}
        {call.truncated ? <p className="mt-1 text-(--fg-tertiary)">{t('chat.agent.tool.truncated')}</p> : null}
    </div>;
}

function Turn({ turn, terminals, onOpenFile }: { turn: AgentTurn; terminals: Terminals; onOpenFile: (path: string) => void }) {
    const { t } = useTranslation();
    // Terminals a tool call shows are not repeated on their own.
    const shown = new Set(turn.tools.flatMap(call => call.content.flatMap(item => item.type === 'terminal' ? [item.terminal_id] : [])));
    return <div className="space-y-2">
        {turn.prompt ? <div className="ml-8 whitespace-pre-wrap break-words rounded bg-(--bg-surface) px-3 py-2 text-(--fg-primary)">{turn.prompt}</div> : null}
        {turn.thoughts ? <details className="text-(--fg-tertiary)">
            <summary className="cursor-pointer">{t('chat.agent.thoughts')}</summary>
            <p className="mt-1 whitespace-pre-wrap break-words">{turn.thoughts}</p>
        </details> : null}
        {turn.plan.length ? <div>
            <p className="font-medium text-(--fg-secondary)">{t('chat.agent.plan')}</p>
            <ol className="ml-4 list-decimal">
                {turn.plan.map((entry, index) => <li key={index} className={entry.status === 'completed' ? 'line-through text-(--fg-tertiary)' : ''}>{entry.content}</li>)}
            </ol>
        </div> : null}
        {turn.tools.length ? <div className="space-y-1">{turn.tools.map(call => <ToolCallCard key={call.id} call={call} terminals={terminals} onOpenFile={onOpenFile} />)}</div> : null}
        {turn.terminals.filter(id => !shown.has(id) && terminals[id]).map(id => <TerminalCard key={id} terminal={terminals[id]} />)}
        {turn.files.some(file => !file.refused) ? <div>
            <p>{t('chat.agent.files.written')}</p>
            {turn.files.filter(file => !file.refused).map(file => <button key={file.path} type="button"
                className="block max-w-full truncate text-left text-(--accent-mention) hover:underline" onClick={() => onOpenFile(file.path)}>{file.path}</button>)}
        </div> : null}
        {turn.files.map(file => file.refused ? <p key={file.path} role="status" className="break-words text-(--state-danger)">
            {t(`chat.agent.files.refused.${file.refused}`, { path: file.path })}
        </p> : null)}
        {turn.text ? <MarkdownRenderer content={turn.text} /> : null}
        {turn.truncated ? <p className="text-(--fg-tertiary)">{t('chat.agent.truncated')}</p> : null}
        {turn.state === 'running' ? <p role="status" className="text-(--fg-tertiary)">{t('chat.agent.working')}</p> : null}
        {turn.state === 'ended' && turn.stopReason && turn.stopReason !== 'end_turn'
            ? <p role="status" className="text-(--fg-tertiary)">{t(`chat.agent.stopReason.${turn.stopReason}`)}</p> : null}
        {turn.state === 'failed' ? <p role="status" className="text-(--state-danger)">
            {turn.error === 'cancelled' ? t('chat.agent.stopReason.cancelled') : t('chat.agent.failed', { reason: t(integrationProbeErrorKey(turn.error)) })}
        </p> : null}
    </div>;
}

function PermissionCard({ name, request, terminals, busy, answer }: { name: string; request: AcpPermission; terminals: Terminals; busy: boolean; answer: (optionId: string | null) => void }) {
    const { t } = useTranslation();
    return <div className="space-y-2 rounded border border-(--accent-ai) p-3">
        <p className="font-medium text-(--fg-primary)">{t('chat.agent.permission.title', { name })}</p>
        <p className="break-words text-(--fg-primary)">{request.tool_call.title || request.tool_call.id}</p>
        {request.tool_call.locations.map((location, index) => <p key={index} className="break-all font-mono">{location.path}</p>)}
        {request.tool_call.content.map((item, index) => <ToolContent key={index} item={item} terminals={terminals} />)}
        <p className="text-(--fg-tertiary)">{t('chat.agent.permission.help')}</p>
        <div className="flex flex-wrap gap-2">
            {request.options.map(option => <button key={option.id} type="button" disabled={busy} onClick={() => answer(option.id)}
                className={`${buttonClass} ${option.kind.startsWith('allow') ? 'border-(--accent-ai) text-(--fg-primary)' : ''}`}>{option.name}</button>)}
            <button type="button" className={buttonClass} disabled={busy} onClick={() => answer(null)}>{t('chat.agent.permission.dismiss')}</button>
        </div>
    </div>;
}

/** A conversation run by an ACP agent. What it reports is shown; Blade runs
 * none of its tools. Files it writes through Blade are listed per turn and
 * reviewed in the editor. */
export function AcpConversationView({ chat, busy, error, send, cancel, answer, authenticate, configure, end, onOpenFile }: {
    chat: AgentChat;
    busy: boolean;
    error: string | null;
    send: (text: string) => void;
    cancel: () => void;
    answer: (requestId: string, optionId: string | null) => void;
    authenticate: (methodId: string) => void;
    configure: (change: AcpConfigChange) => void;
    end: () => void;
    onOpenFile: (path: string) => void;
}) {
    const { t } = useTranslation();
    const [draft, setDraft] = useState('');
    const [stopping, setStopping] = useState(false);
    const bottom = useRef<HTMLDivElement>(null);
    const name = chat.status.name;
    const agent = chat.status.snapshot.agent;
    const running = runningTurn(chat);
    const live = !chat.exited;
    const canSend = live && chat.status.ready && !running && !busy && draft.trim().length > 0;
    useEffect(() => { if (!running) setStopping(false); }, [running]);
    useEffect(() => { bottom.current?.scrollIntoView?.({ block: 'end' }); }, [chat.turns, chat.permissions]);
    const submit = () => {
        if (!canSend) return;
        send(draft.trim());
        setDraft('');
    };
    const agentMethods = chat.status.snapshot.auth_methods.filter(method => !method.terminal);
    const terminalMethods = chat.status.snapshot.auth_methods.filter(method => method.terminal);
    return <div className="flex min-h-0 flex-1 flex-col text-xs text-(--fg-secondary)">
        <div className="shrink-0 border-b border-(--border-subtle) px-4 py-2">
            <p className="font-medium text-(--fg-primary)">{t('chat.agent.runBy', { name })}{agent ? ` · ${agent.title || agent.name} ${agent.version}` : ''}</p>
            {chat.title ? <p className="truncate">{chat.title}</p> : null}
            <AcpConfigControls name={name} config={chat.status.config} disabled={!live || !chat.status.ready || busy}
                running={Boolean(running)} configure={configure} />
            <p className="text-(--fg-tertiary)">{t('chat.agent.reportsHelp')}</p>
            {chat.usage ? <p className="text-(--fg-tertiary)">{t('chat.agent.usage', { used: chat.usage.used, size: chat.usage.size })}</p> : null}
            <AcpForwarding forwarding={chat.status.forwarding} />
        </div>
        <div className="min-h-0 flex-1 space-y-4 overflow-auto px-4 py-3">
            {!chat.status.ready && live ? <div className="space-y-2 rounded border border-(--border-default) p-3">
                <p className="font-medium text-(--fg-primary)">{t('chat.agent.signIn.title', { name })}</p>
                <p>{agentMethods.length ? t('chat.agent.signIn.help') : t('chat.agent.signIn.none')}</p>
                <div className="flex flex-wrap gap-2">
                    {agentMethods.map(method => <button key={method.id} type="button" className={buttonClass} disabled={busy}
                        title={method.description ?? undefined} onClick={() => authenticate(method.id)}>{method.name}</button>)}
                </div>
                {terminalMethods.map(method => <p key={method.id} className="text-(--fg-tertiary)">{t('chat.agent.signIn.terminal', { method: method.name })}</p>)}
            </div> : null}
            {chat.turns.map(turn => <Turn key={turn.turn} turn={turn} terminals={chat.terminals} onOpenFile={onOpenFile} />)}
            {chat.permissions.map(request => <PermissionCard key={request.request_id} name={name} request={request} terminals={chat.terminals} busy={busy}
                answer={optionId => answer(request.request_id, optionId)} />)}
            {chat.exited ? <p role="status" className="text-(--fg-tertiary)">
                {chat.exited.error ? t('chat.agent.exitedError', { reason: t(integrationProbeErrorKey(chat.exited.error)) }) : t('chat.agent.exited')}
            </p> : null}
            <div ref={bottom} />
        </div>
        {error ? <p role="alert" className="shrink-0 px-4 pb-1 text-(--state-danger)">{t(error)}</p> : null}
        <form className="shrink-0 space-y-2 border-t border-(--border-subtle) p-3" onSubmit={event => { event.preventDefault(); submit(); }}>
            <textarea value={draft} onChange={event => setDraft(event.target.value)} rows={3} disabled={!live}
                aria-label={t('chat.agent.placeholder', { name })} placeholder={t('chat.agent.placeholder', { name })}
                className="w-full resize-y rounded border border-(--border-default) bg-(--bg-app) px-2 py-1.5 text-sm text-(--fg-primary) disabled:opacity-50"
                onKeyDown={event => {
                    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); submit(); }
                }} />
            <div className="flex flex-wrap justify-end gap-2">
                {live ? <button type="button" className={buttonClass} disabled={busy} onClick={end}>{t('chat.agent.end')}</button> : null}
                {running && live ? <button type="button" className={buttonClass} disabled={stopping}
                    onClick={() => { setStopping(true); cancel(); }}>{stopping ? t('chat.agent.stopping') : t('chat.agent.stop')}</button> : null}
                <button type="submit" className={buttonClass} disabled={!canSend}>{t('chat.agent.send')}</button>
            </div>
        </form>
    </div>;
}
