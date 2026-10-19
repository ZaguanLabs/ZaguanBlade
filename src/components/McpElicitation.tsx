import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { elicitationContent, initialValues, type ElicitationRequest, type FieldValue, type McpElicitationField } from '../utils/mcpElicitation';

export type ElicitationAnswer = 'accept' | 'decline' | 'cancel';
const buttonClass = 'rounded border border-(--border-default) px-3 py-1.5 hover:bg-(--bg-surface-hover) disabled:opacity-50';
const inputClass = 'w-full rounded border border-(--border-default) bg-(--bg-input) px-2 py-1 text-(--fg-primary)';

function FieldInput({ field, value, onChange, invalid }: { field: McpElicitationField; value: FieldValue; onChange: (value: FieldValue) => void; invalid: boolean }) {
    const { t } = useTranslation();
    const id = useId();
    const label = <span className="text-(--fg-primary)">{field.title || field.name}{field.required ? <span className="text-(--state-danger)"> *</span> : null}</span>;
    const help = field.description ? <span className="block text-(--fg-tertiary)">{field.description}</span> : null;
    const error = invalid ? <span role="alert" className="block text-(--state-danger)">{t('mcpChat.elicitation.invalid')}</span> : null;
    if (field.kind === 'boolean') {
        return <label className="flex items-start gap-2">
            <input type="checkbox" checked={value === true} onChange={event => onChange(event.target.checked)} />
            <span>{label}{help}</span>
        </label>;
    }
    if (field.kind === 'choice' && field.multiple) {
        const selected = Array.isArray(value) ? value : [];
        return <fieldset className="space-y-1">
            <legend>{label}</legend>{help}
            {field.options.map(option => <label key={option.value} className="flex gap-2">
                <input type="checkbox" checked={selected.includes(option.value)}
                    onChange={event => onChange(event.target.checked ? [...selected, option.value] : selected.filter(item => item !== option.value))} />
                {option.label}
            </label>)}
            {error}
        </fieldset>;
    }
    return <label htmlFor={id} className="block space-y-1">
        {label}{help}
        {field.kind === 'choice'
            ? <select id={id} className={inputClass} value={String(value)} onChange={event => onChange(event.target.value)}>
                {!field.required ? <option value="">{t('mcpChat.elicitation.none')}</option> : null}
                {field.options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
            : <input id={id} className={inputClass} value={String(value)} spellCheck={false}
                type={field.kind === 'number' ? 'number' : field.format === 'date' ? 'date' : field.format === 'email' ? 'email' : field.format === 'uri' ? 'url' : 'text'}
                step={field.kind === 'number' && field.integer ? 1 : undefined}
                maxLength={field.kind === 'text' ? field.max_length : undefined}
                placeholder={field.kind === 'text' && field.format === 'date-time' ? t('mcpChat.elicitation.dateTimeFormat') : undefined}
                onChange={event => onChange(event.target.value)} />}
        {error}
    </label>;
}

/** A request for input from an MCP server or an ACP agent. Their text is
 * shown as text; nothing is sent until the user chooses an answer. `title`,
 * `help` and `urlHelp` say who asks. */
export function ElicitationCard({ request, title, help, urlHelp, busy, respond }: {
    request: ElicitationRequest;
    title: string;
    help: string;
    urlHelp: string;
    busy: boolean;
    respond: (answer: ElicitationAnswer, content?: Record<string, unknown>) => void;
}) {
    const { t } = useTranslation();
    const [values, setValues] = useState(() => request.mode === 'form' ? initialValues(request.fields) : {});
    const [invalid, setInvalid] = useState<string[]>([]);
    const submit = () => {
        if (request.mode !== 'form') return;
        const result = elicitationContent(request.fields, values);
        setInvalid(result.invalid);
        if (!result.invalid.length) respond('accept', result.content);
    };
    return <div className="mt-3 space-y-2 rounded border border-(--border-default) p-3">
        <p className="font-medium text-(--fg-primary)">{title}</p>
        <p className="whitespace-pre-wrap break-words">{request.message}</p>
        {request.mode === 'url' ? <>
            <p>{urlHelp}</p>
            <p className="break-all rounded bg-(--bg-app) p-2 font-mono">{request.url}</p>
        </> : <form className="space-y-2" onSubmit={event => { event.preventDefault(); submit(); }}>
            {request.fields.map(field => <FieldInput key={field.name} field={field} value={values[field.name] ?? ''} invalid={invalid.includes(field.name)}
                onChange={value => setValues(current => ({ ...current, [field.name]: value }))} />)}
        </form>}
        <p className="text-(--fg-tertiary)">{help}</p>
        <div className="flex flex-wrap gap-2">
            <button type="button" className={buttonClass} disabled={busy} onClick={() => respond('cancel')}>{t('mcpChat.elicitation.cancel')}</button>
            <button type="button" className={buttonClass} disabled={busy} onClick={() => respond('decline')}>{t('mcpChat.elicitation.decline')}</button>
            <button type="button" className={buttonClass} disabled={busy} onClick={() => request.mode === 'url' ? respond('accept') : submit()}>
                {t(request.mode === 'url' ? 'mcpChat.elicitation.open' : 'mcpChat.elicitation.submit')}
            </button>
        </div>
    </div>;
}
