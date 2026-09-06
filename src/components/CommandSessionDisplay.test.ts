import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import en from '../../public/locales/en/translation.json';
import es from '../../public/locales/es/translation.json';
import appI18n from '../i18n';
import type { ToolCall } from '../types/chat';
import { getCommandSessionAction, parseCommandSessionResult } from '../utils/commandSession';
import { deriveChatWorkEntries } from '../utils/chatTimeline';
import { ToolCallDisplay } from './ToolCallDisplay';

const resources = { en: { translation: en }, es: { translation: es } };
const locales = Object.fromEntries(['en', 'es'].map((lng) => {
    const i18n = createInstance();
    void i18n.init({ lng, resources, initAsync: false, interpolation: { escapeValue: false } });
    return [lng, i18n];
}));

function envelope(state: string, output = '(no new output)', seconds = 12) {
    return `Wall time: ${seconds} seconds\nProcess ${state}\nOutput:\n${output}\n`;
}

function renderSession(args: unknown, result?: string, status: NonNullable<ToolCall['status']> = 'complete', language = 'en') {
    return renderToStaticMarkup(createElement(I18nextProvider, { i18n: locales[language] },
        createElement(ToolCallDisplay, {
            toolCall: { id: 'call-session', type: 'function', function: { name: 'command_session', arguments: JSON.stringify(args) } },
            result,
            status,
        })));
}

function workEntry(args: unknown, result?: string, status: NonNullable<ToolCall['status']> = 'complete') {
    appI18n.addResourceBundle('en', 'translation', en, true, true);
    return deriveChatWorkEntries([{
        id: 'assistant-session', role: 'Assistant', content: '',
        tool_calls: [{ id: 'session-call', type: 'function', function: { name: 'command_session', arguments: JSON.stringify(args) }, result, status }],
    }])[0];
}

describe('command session parsing', () => {
    it('distinguishes polling, input, all backend interrupt spellings, and kill precedence', () => {
        assert.equal(getCommandSessionAction(null), 'poll');
        assert.equal(getCommandSessionAction({ input: '', kill: false }), 'poll');
        assert.equal(getCommandSessionAction({ input: 123 }), 'poll');
        assert.equal(getCommandSessionAction({ input: 'yes\n' }), 'write');
        assert.equal(getCommandSessionAction({ input: ' ' }), 'write');
        for (const input of ['\u0003', '\\x03', '\\u0003', '^C']) {
            assert.equal(getCommandSessionAction({ input }), 'interrupt');
            assert.equal(getCommandSessionAction({ input, kill: true }), 'kill');
        }
    });

    it('parses running sessions, elapsed seconds, and empty output', () => {
        assert.deepEqual(parseCommandSessionResult(envelope('running with session ID build-1')), {
            state: 'running', sessionId: 'build-1', elapsedSeconds: 12, output: '',
        });
    });

    it('parses successful and unsuccessful exits, including negative codes and CRLF', () => {
        for (const exitCode of [0, 1, -1, 130]) {
            assert.deepEqual(parseCommandSessionResult(envelope(`exited with code ${exitCode}`, 'done', 0).replace(/\n/g, '\r\n')), {
                state: 'exited', exitCode, elapsedSeconds: 0, output: 'done\n',
            });
        }
    });

    it('never interprets process-like lines inside output as status', () => {
        const output = 'Process exited with code 1\nOutput:\n  indented [slug]';
        const parsed = parseCommandSessionResult(envelope('running with session ID build-1', output));
        assert.equal(parsed.state, 'running');
        assert.equal(parsed.exitCode, undefined);
        assert.equal(parsed.output, `${output}\n`);
    });

    it('preserves unknown, malformed, and error results without throwing', () => {
        for (const result of [undefined, '', 'Unknown session id "missing".', '{invalid json', 'Process exited with code 0']) {
            assert.deepEqual(parseCommandSessionResult(result), { state: 'unknown', output: result ?? '' });
        }
    });
});

describe('command session chat rendering', () => {
    it('renders a friendly polling card rather than the raw tool name or envelope', () => {
        const html = renderSession({ session_id: 'build-1', input: '', wait_ms: 3000 }, envelope('running with session ID build-1'));
        assert.match(html, /Checking command progress/);
        assert.match(html, /Still running/);
        assert.match(html, /Session: build-1/);
        assert.match(html, /Elapsed: 12 s/);
        assert.match(html, /No new output/);
        assert.doesNotMatch(html, /command_session|Wall time:|Process running|wait_ms|Complete|<details/);
    });

    it('uses localized labels for input, interrupts, and stopping without exposing input', () => {
        const cases = [
            [{ input: 'secret-token\n' }, 'Sending command input', 'Enviando datos al comando'],
            [{ input: '\\x03' }, 'Interrupting command', 'Interrumpiendo el comando'],
            [{ kill: true, input: '\\x03' }, 'Stopping background command', 'Deteniendo el comando en segundo plano'],
        ] as const;
        for (const [args, english, spanish] of cases) {
            for (const [language, label] of [['en', english], ['es', spanish]]) {
                const html = renderSession({ session_id: 'build-1', ...args }, undefined, 'executing', language);
                assert.ok(html.includes(label));
                assert.doesNotMatch(html, /secret-token|command_session|\\x03/);
            }
        }
    });

    it('renders Spanish process metadata and empty-output text', () => {
        const html = renderSession({ session_id: 'build-1' }, envelope('running with session ID build-1'), 'complete', 'es');
        for (const label of ['Comprobando el progreso del comando', 'Sigue en ejecución', 'Sesión: build-1', 'Tiempo transcurrido: 12 s', 'Sin nueva salida']) {
            assert.ok(html.includes(label));
        }
    });

    it('preserves multiline output in a collapsed terminal block and strips ANSI', () => {
        const html = renderSession({ session_id: 'build-1' }, envelope('exited with code 0', '\u001b[32mdone\u001b[0m\n  route [slug]\n<script>alert(1)</script>'));
        assert.match(html, /Finished/);
        assert.match(html, /Exit 0/);
        assert.match(html, /<details /);
        assert.doesNotMatch(html, /<details[^>]* open|\u001b|<script>/);
        assert.match(html, /done\n {2}route \[slug\]/);
        assert.match(html, /&lt;script&gt;/);
    });

    it('shows nonzero exits as failures even when the poll itself completed', () => {
        const html = renderSession({ session_id: 'build-1' }, envelope('exited with code 1', 'test failed'));
        assert.match(html, /Failed/);
        assert.match(html, /Exit 1/);
        assert.match(html, /<details[^>]* open/);
        assert.match(html, /test failed/);
        assert.doesNotMatch(html, /Finished/);
    });

    it('preserves session errors and does not invent a successful process result', () => {
        const html = renderSession({ session_id: 'missing' }, 'Unknown session id "missing".', 'error');
        assert.match(html, /Failed/);
        assert.match(html, /Unknown session id/);
        assert.match(html, /<details[^>]* open/);
        assert.doesNotMatch(html, /Finished|Exit 0/);
    });

    it('handles pending, executing, skipped, and malformed arguments safely', () => {
        for (const [status, label] of [['pending', 'Pending'], ['executing', 'Executing'], ['skipped', 'Skipped']] as const) {
            const html = renderSession(null, undefined, status);
            assert.ok(html.includes(label));
            assert.doesNotMatch(html, /Finished|Exit |No new output/);
        }
        const html = renderToStaticMarkup(createElement(I18nextProvider, { i18n: locales.en },
            createElement(ToolCallDisplay, {
                toolCall: { id: 'malformed', type: 'function', function: { name: 'command_session', arguments: '{"session_id":' } },
                status: 'pending',
            })));
        assert.match(html, /Checking command progress/);
    });

    it('uses persisted tool results when no separate result prop is supplied', () => {
        const html = renderToStaticMarkup(createElement(I18nextProvider, { i18n: locales.en },
            createElement(ToolCallDisplay, {
                toolCall: { id: 'saved', type: 'function', function: { name: 'command_session', arguments: '{}' }, result: envelope('exited with code 0') },
                status: 'complete',
            })));
        assert.match(html, /Finished/);
    });
});

describe('command session work log', () => {
    it('uses the friendly action and session rather than raw result metadata', () => {
        const entry = workEntry({ session_id: 'build-1' }, envelope('running with session ID build-1'));
        assert.equal(entry.label, 'Checking command progress');
        assert.equal(entry.detail, 'Session: build-1');
        assert.equal(entry.status, 'complete'); // A finished poll must not keep the chat busy forever.
        assert.equal(entry.tone, 'tool');
    });

    it('recognizes interrupt actions and unsuccessful exits', () => {
        const entry = workEntry({ session_id: 'build-1', input: '^C' }, envelope('exited with code 130'));
        assert.equal(entry.label, 'Interrupting command');
        assert.equal(entry.tone, 'error');
    });

    it('does not leak raw input or malformed result text into the summary', () => {
        const entry = workEntry({ input: 'private input' }, 'raw result', 'error');
        assert.equal(entry.label, 'Sending command input');
        assert.equal(entry.detail, undefined);
        assert.equal(entry.tone, 'error');
    });
});
