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

export function readDebugFlag(name: string): boolean {
    if (typeof window === 'undefined') {
        return false;
    }

    const envValue = window.__ZBLADE_DEBUG_FLAGS__?.[name];
    if (envValue === 'true') {
        return true;
    }
    if (envValue === 'false') {
        return false;
    }

    const queryValue = new URLSearchParams(window.location.search).get(name);
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

export function readDebugSurfaceFlag(name: string): boolean {
    if (typeof window === 'undefined') {
        return false;
    }

    const envKey = `disable${name.slice('disable'.length).toUpperCase()}`;
    const envValue = window.__ZBLADE_DEBUG_FLAGS__?.[envKey];
    if (envValue === 'true') {
        return true;
    }
    if (envValue === 'false') {
        return false;
    }

    return readDebugFlag(name);
}
