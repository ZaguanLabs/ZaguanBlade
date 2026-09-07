import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { McpCatalog, McpCatalogTool } from '../../types/integrations';
import { catalogPage } from '../../utils/integrationCatalog';

function ToolCard({ tool }: { tool: McpCatalogTool }) {
    const { t } = useTranslation();
    const [expanded, setExpanded] = useState(false);
    const detailsId = useId();
    const definition = tool.definition;
    return <li className="space-y-2 rounded-md border border-(--border-default) p-3">
        <h5 className="break-all font-mono text-sm text-(--fg-primary)"><bdi>{definition.name}</bdi></h5>
        {definition.title ? <p className="break-words text-sm text-(--fg-secondary)">{definition.title}</p> : null}
        {definition.description ? <p className="max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs text-(--fg-secondary)">{definition.description}</p> : null}
        <button type="button" className="text-xs text-(--fg-secondary) underline underline-offset-2" aria-expanded={expanded} aria-controls={detailsId} onClick={() => setExpanded(value => !value)}>
            {t('settings.integrations.catalog.details')}
        </button>
        {expanded ? <div id={detailsId} className="space-y-2">
            <p className="break-all text-xs text-(--fg-tertiary)">{t('settings.integrations.catalog.alias', { alias: tool.alias })}</p>
            <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-all rounded-md bg-(--bg-input) p-2 text-xs text-(--fg-secondary)">{JSON.stringify(definition, null, 2)}</pre>
        </div> : null}
    </li>;
}

/** Server text is plain escaped text. Schemas, icon URLs and resource references
 * are inspected as data; rendering never loads third-party content. */
export function IntegrationToolCatalog({ catalog, serverName }: { catalog: McpCatalog; serverName: string }) {
    const { t } = useTranslation();
    const [query, setQuery] = useState('');
    const [page, setPage] = useState(0);
    const searchId = useId();
    const result = catalogPage(catalog.tools, query, page);
    return <section className="space-y-3 rounded-md border border-(--border-default) p-3">
        <h4 className="text-sm font-semibold text-(--fg-primary)">{t('settings.integrations.catalog.title', { name: serverName })}</h4>
        <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.catalog.help')}</p>
        <label htmlFor={searchId} className="block text-xs text-(--fg-secondary)">{t('settings.integrations.catalog.search')}</label>
        <input id={searchId} type="search" maxLength={256} value={query} onChange={event => { setQuery(event.target.value); setPage(0); }}
            className="w-full rounded-md border border-(--border-default) bg-(--bg-input) px-3 py-2 text-sm text-(--fg-primary)" />
        <p role="status" className="text-xs text-(--fg-secondary)">{t('settings.integrations.catalog.count', { count: result.total })}</p>
        {catalog.tools.length === 0 ? <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.catalog.empty')}</p>
            : result.total === 0 ? <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.catalog.noMatches')}</p> : null}
        <ul className="space-y-2">{result.tools.map(tool => <ToolCard key={tool.alias} tool={tool} />)}</ul>
        {result.pages > 1 ? <nav aria-label={t('settings.integrations.catalog.pages')} className="flex items-center gap-3 text-xs text-(--fg-secondary)">
            <button type="button" className="underline disabled:opacity-50" disabled={result.page === 0} onClick={() => setPage(result.page - 1)}>{t('settings.integrations.catalog.previous')}</button>
            <span>{t('settings.integrations.catalog.page', { page: result.page + 1, pages: result.pages })}</span>
            <button type="button" className="underline disabled:opacity-50" disabled={result.page + 1 >= result.pages} onClick={() => setPage(result.page + 1)}>{t('settings.integrations.catalog.next')}</button>
        </nav> : null}
    </section>;
}
