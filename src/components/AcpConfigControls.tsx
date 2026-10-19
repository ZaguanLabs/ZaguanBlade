import { useTranslation } from 'react-i18next';
import type { AcpConfigChange, AcpConfigChoice, AcpSessionConfig } from '../types/acp';

const selectClass = 'max-w-56 truncate rounded border border-(--border-default) bg-(--bg-app) px-1.5 py-0.5 text-xs text-(--fg-primary) disabled:opacity-50';

function Choices({ choices }: { choices: AcpConfigChoice[] }) {
    return <>{choices.map(choice => <option key={choice.value} value={choice.value} title={choice.description ?? undefined}>{choice.name}</option>)}</>;
}

/** The settings the agent offers for its session (model, effort, mode…), as it
 * declares them. A change goes to the agent, which returns the new state. */
export function AcpConfigControls({ name, config, disabled, running, configure }: {
    name: string;
    config?: AcpSessionConfig;
    disabled: boolean;
    running: boolean;
    configure: (change: AcpConfigChange) => void;
}) {
    const { t } = useTranslation();
    if (!config || (!config.options.length && !config.modes)) return null;
    const locked = disabled || running;
    return <div role="group" aria-label={t('chat.agent.config.label', { name })}
        title={running ? t('chat.agent.config.duringTurn') : undefined}
        className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1">
        {config.options.map(option => <label key={option.id} className="flex items-center gap-1" title={option.description ?? undefined}>
            {option.type === 'boolean' ? <>
                <input type="checkbox" checked={option.current_value} disabled={locked}
                    onChange={event => configure({ type: 'boolean', config_id: option.id, value: event.target.checked })} />
                <span>{option.name}</span>
            </> : <>
                <span>{option.name}</span>
                <select className={selectClass} value={option.current_value} disabled={locked}
                    onChange={event => configure({ type: 'select', config_id: option.id, value: event.target.value })}>
                    {option.groups.some(group => group.choices.some(choice => choice.value === option.current_value))
                        ? null : <option value={option.current_value} disabled>{option.current_value}</option>}
                    {option.groups.map((group, index) => group.name
                        ? <optgroup key={index} label={group.name}><Choices choices={group.choices} /></optgroup>
                        : <Choices key={index} choices={group.choices} />)}
                </select>
            </>}
        </label>)}
        {config.modes ? <label className="flex items-center gap-1">
            <span>{t('chat.agent.config.mode')}</span>
            <select className={selectClass} value={config.modes.current_mode_id} disabled={locked}
                onChange={event => configure({ type: 'mode', mode_id: event.target.value })}>
                {config.modes.modes.some(mode => mode.value === config.modes?.current_mode_id)
                    ? null : <option value={config.modes.current_mode_id} disabled>{config.modes.current_mode_id}</option>}
                <Choices choices={config.modes.modes} />
            </select>
        </label> : null}
    </div>;
}
