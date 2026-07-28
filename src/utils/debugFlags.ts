declare global {
    interface Window {
        __ZBLADE_DEBUG_FLAGS__?: Record<string, string | undefined>;
    }
}

export function parseBooleanFlag(value: string | undefined): string | undefined {
    if (!value) {
        return undefined;
    }

    if (value === '1' || value.toLowerCase() === 'true') {
        return 'true';
    }

    if (value === '0' || value.toLowerCase() === 'false') {
        return 'false';
    }

    return undefined;
}

// Flag resolution reads the query string and localStorage, both of which are
// comparatively expensive (localStorage.getItem is a synchronous, disk-backed
// call). Reads happen on hot paths — several per Layout/ChatViewport render and
// once per recordDebugPerf call, which fires on every streamed commit, every
// scroll frame and every terminal resize frame — so results are memoized.
//
// The cache is keyed on the identity of window.__ZBLADE_DEBUG_FLAGS__. Startup
// installs a build-time flag object and then *replaces* it with a merged object
// once the runtime flags arrive from the backend (see main.tsx). Keying on
// identity makes that swap invalidate the cache automatically, so flags read
// before the merge (e.g. startupMarks) cannot pin a stale value.
const UNRESOLVED_FLAG_SOURCE = Symbol('unresolved-flag-source');
let cachedFlagSource: Record<string, string | undefined> | undefined | typeof UNRESOLVED_FLAG_SOURCE =
    UNRESOLVED_FLAG_SOURCE;
const flagCache = new Map<string, boolean>();

let cachedSearch: string | null = null;
let cachedSearchParams: URLSearchParams | null = null;

function getSearchParams(): URLSearchParams {
    const search = window.location.search;
    if (cachedSearchParams === null || cachedSearch !== search) {
        cachedSearch = search;
        cachedSearchParams = new URLSearchParams(search);
    }
    return cachedSearchParams;
}

function getFlagCache(): Map<string, boolean> {
    const source = window.__ZBLADE_DEBUG_FLAGS__;
    if (cachedFlagSource !== source) {
        cachedFlagSource = source;
        flagCache.clear();
    }
    return flagCache;
}

function resolveDebugFlag(name: string): boolean {
    const envValue = window.__ZBLADE_DEBUG_FLAGS__?.[name];
    if (envValue === 'true') {
        return true;
    }
    if (envValue === 'false') {
        return false;
    }

    const queryValue = getSearchParams().get(name);
    if (queryValue === '1' || queryValue === 'true') {
        return true;
    }
    if (queryValue === '0' || queryValue === 'false') {
        return false;
    }

    try {
        const storageValue = window.localStorage.getItem(`zblade.debug.${name}`);
        return storageValue === '1' || storageValue === 'true';
    } catch {
        return false;
    }
}

export function readDebugFlag(name: string): boolean {
    if (typeof window === 'undefined') {
        return false;
    }

    const cache = getFlagCache();
    const cached = cache.get(name);
    if (cached !== undefined) {
        return cached;
    }

    const resolved = resolveDebugFlag(name);
    cache.set(name, resolved);
    return resolved;
}

export function readDebugSurfaceFlag(name: string): boolean {
    if (typeof window === 'undefined') {
        return false;
    }

    const cache = getFlagCache();
    const cacheKey = `surface:${name}`;
    const cached = cache.get(cacheKey);
    if (cached !== undefined) {
        return cached;
    }

    const envKey = `disable${name.slice('disable'.length).toUpperCase()}`;
    const envValue = window.__ZBLADE_DEBUG_FLAGS__?.[envKey];
    const resolved = envValue === 'true'
        ? true
        : envValue === 'false'
            ? false
            : readDebugFlag(name);

    cache.set(cacheKey, resolved);
    return resolved;
}
