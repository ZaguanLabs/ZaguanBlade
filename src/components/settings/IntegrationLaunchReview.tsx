import { useTranslation } from 'react-i18next';
import type { IntegrationLaunchReview } from '../../types/integrations';
import { AcpForwarding } from '../AcpForwarding';

/** Approval details for a one-use launch or connection ticket. */
export function LaunchReviewDetails({ review, purpose }: { review: IntegrationLaunchReview | null; purpose: 'test' | 'connection' | 'agent' }) {
    const { t } = useTranslation();
    if (!review) return null;
    if (review.kind === 'http') return <div className="space-y-3">
        <p>{t(`settings.integrations.${purpose}.reviewHelpHttp`)}</p>
        {review.insecure ? <p role="alert" className="text-(--state-danger)">{t('settings.integrations.review.insecure')}</p> : null}
        <dl className="max-h-60 overflow-auto break-all">
            <dt>{t('settings.integrations.url')}</dt><dd className="font-mono text-xs">{review.url}</dd>
            <dt className="mt-2">{t('settings.integrations.review.headers')}</dt>
            <dd className="font-mono text-xs whitespace-pre-wrap">{review.header_names.length ? review.header_names.join('\n') : t('settings.integrations.review.noHeaders')}</dd>
            {review.oauth ? <><dt className="mt-2">{t('settings.integrations.review.authentication')}</dt><dd className="text-xs">{t('settings.integrations.review.oauth')}</dd></> : null}
        </dl>
    </div>;
    return <div className="space-y-3">
        <p>{t(`settings.integrations.${purpose}.reviewHelp`)}</p>
        <dl className="max-h-60 overflow-auto break-all">
            <dt>{t('settings.integrations.command')}</dt><dd className="font-mono text-xs">{review.executable}</dd>
            <dt className="mt-2">{t('settings.integrations.arguments')}</dt><dd className="font-mono text-xs whitespace-pre-wrap">{JSON.stringify(review.args, null, 2)}</dd>
            <dt className="mt-2">{t('settings.integrations.directory')}</dt><dd className="font-mono text-xs">{review.cwd}</dd>
        </dl>
        {purpose === 'agent' ? <AcpForwarding forwarding={review.forwarding} review /> : null}
    </div>;
}
