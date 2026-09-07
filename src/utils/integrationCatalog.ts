import type { McpCatalogTool } from '../types/integrations';

const PAGE_SIZE = 20;

export function catalogPage(tools: readonly McpCatalogTool[], query: string, requestedPage: number) {
    const term = query.trim().toLowerCase();
    const matching = term ? tools.filter(tool => [tool.definition.name, tool.definition.title, tool.definition.description]
        .some(value => value?.toLowerCase().includes(term))) : tools;
    const pages = Math.max(1, Math.ceil(matching.length / PAGE_SIZE));
    const page = Math.max(0, Math.min(Number.isFinite(requestedPage) ? Math.floor(requestedPage) : 0, pages - 1));
    return { tools: matching.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE), total: matching.length, page, pages };
}
