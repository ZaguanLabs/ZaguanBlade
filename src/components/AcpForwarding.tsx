import { useTranslation } from 'react-i18next';
import type { AcpForwardingView } from '../types/integrations';
import { integrationProbeErrorKey } from '../utils/integrationProbe';

/** The MCP servers an agent is given and those it is not, with the reason.
 * `review` is the launch approval; otherwise it is the live session. */
export function AcpForwarding({ forwarding, review = false }: { forwarding?: AcpForwardingView | null; review?: boolean }) {
    const { t } = useTranslation();
    if (!forwarding || (!forwarding.shared.length && !forwarding.skipped.length)) return null;
    const { shared, skipped } = forwarding;
    const details = <>
        {review ? <p>{t('settings.integrations.forwarding.reviewHelp')}</p> : null}
        {shared.length ? <ul className="space-y-1">
            {shared.map(server => <li key={server.id}>
                <span className="font-medium text-(--fg-primary)">{server.name}</span>
                {' · '}{t(`settings.integrations.forwarding.transport.${server.transport}`)}
                {server.secrets.length ? <span className="block font-mono text-xs">{t('settings.integrations.forwarding.secrets', { names: server.secrets.join(', ') })}</span> : null}
            </li>)}
        </ul> : null}
        {review && shared.some(server => server.transport === 'http') ? <p className="text-(--fg-tertiary)">{t('settings.integrations.forwarding.httpNote')}</p> : null}
        {skipped.length ? <>
            <p className="font-medium text-(--fg-primary)">{t('settings.integrations.forwarding.skippedTitle')}</p>
            <ul className="space-y-1">
                {skipped.map(server => <li key={server.id}>
                    {server.name}: {t(`settings.integrations.forwarding.skipped.${server.reason}`, { reason: t(integrationProbeErrorKey(server.error)) })}
                </li>)}
            </ul>
        </> : null}
    </>;
    if (review) return <section className="space-y-2">
        <p className="font-medium text-(--fg-primary)">{t('settings.integrations.forwarding.title')}</p>
        {shared.length ? null : <p>{t('settings.integrations.forwarding.summaryNone')}</p>}
        {details}
    </section>;
    return <details className="text-(--fg-tertiary)">
        <summary className="cursor-pointer">{shared.length
            ? t('settings.integrations.forwarding.summary', { count: shared.length, names: shared.map(server => server.name).join(', ') })
            : t('settings.integrations.forwarding.summaryNone')}</summary>
        <div className="mt-1 space-y-2">{details}</div>
    </details>;
}
