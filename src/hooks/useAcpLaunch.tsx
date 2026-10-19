import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import type { AcpAgentStatus } from '../types/acp';
import type { IntegrationDefinition, IntegrationLaunchReview } from '../types/integrations';
import { AcpStartRequest } from '../utils/acpConversation';
import { integrationProbeErrorKey } from '../utils/integrationProbe';
import { LaunchReviewDetails } from '../components/settings/IntegrationLaunchReview';
import { ConfirmModal } from '../components/ui/Modal';

/** Prepares an agent launch, shows the exact program for approval, then
 * starts it: in a new conversation, or reconnected to `conversationId`. */
export function useAcpLaunch(onStarted: (status: AcpAgentStatus) => void) {
    const { t } = useTranslation();
    const [error, setError] = useState<string | null>(null);
    const [review, setReview] = useState<{ name: string; review: IntegrationLaunchReview } | null>(null);
    const [starting, setStarting] = useState<string | null>(null);
    const request = useRef<AcpStartRequest | null>(null);
    const target = useRef<string | undefined>(undefined);

    useEffect(() => () => { request.current?.dispose(); request.current = null; }, []);

    const reset = useCallback(() => { request.current?.dispose(); request.current = null; setReview(null); setStarting(null); }, []);
    const prepare = useCallback(async (entry: IntegrationDefinition, revision: string, workspaceRoot: string, conversationId?: string) => {
        if (request.current) return;
        const current = new AcpStartRequest(invoke);
        request.current = current;
        target.current = conversationId;
        setError(null);
        try {
            const next = await current.prepare(entry.id, revision, workspaceRoot);
            if (request.current === current && next) setReview({ name: entry.name, review: next });
        } catch (failure) {
            if (request.current === current) { setError(integrationProbeErrorKey(failure)); reset(); }
        }
    }, [reset]);
    const start = async () => {
        const current = request.current;
        if (!current || !review) return;
        setStarting(review.name); setReview(null);
        try {
            const status = await current.start(target.current);
            if (request.current === current && status) onStarted(status);
        } catch (failure) {
            if (request.current === current) setError(integrationProbeErrorKey(failure));
        } finally {
            if (request.current === current) { request.current = null; setStarting(null); }
        }
    };
    const modal = <ConfirmModal isOpen={!!review} title={review ? t('chat.agent.reviewTitle', { name: review.name }) : ''}
        confirmLabel={t('chat.agent.allow')} onConfirm={() => void start()} onCancel={reset}
        message={<LaunchReviewDetails review={review?.review ?? null} purpose="agent" />} />;
    return { prepare, starting, error, setError, modal };
}
