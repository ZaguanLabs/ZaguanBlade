#!/usr/bin/env bun
/**
 * Pin benchmark corpora hashes in the performance manifest.
 *
 * For each configured corpus this script computes a stable SHA-256 hash:
 *   - files are hashed directly;
 *   - directories are hashed from a sorted JSON manifest of (relative_path, sha256) entries.
 *
 * Output is written back to the manifest JSON so measurement scripts can
 * reproduce the exact corpus that produced each result.
 *
 * Usage:
 *   bun scripts/benchmark/pin-corpora.ts --manifest=benchmark-results/manifest.json
 */

import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, statSync, readdirSync, existsSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

interface CorpusConfig {
    name: string;
    path: string;
}

const CORPORA: CorpusConfig[] = [
    { name: 'tiny', path: 'benchmarks/corpora/tiny' },
    { name: 'medium', path: 'benchmarks/corpora/medium' },
    { name: 'large_mixed', path: 'benchmarks/corpora/large_mixed' },
    { name: 'git_heavy', path: 'benchmarks/corpora/git_heavy' },
    { name: 'long_chat', path: 'benchmarks/corpora/long_chat.json' },
    { name: 'ui_stress', path: 'benchmarks/corpora/ui_stress' },
];

function parseArgs(): { manifest: string } {
    const manifest =
        process.argv.find((a) => a.startsWith('--manifest='))?.slice('--manifest='.length) ??
        'benchmark-results/manifest.json';
    return { manifest };
}

function sha256File(path: string): string {
    const data = readFileSync(path);
    return createHash('sha256').update(data).digest('hex');
}

function* walkDir(dir: string): Generator<string> {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) {
            yield* walkDir(full);
        } else if (entry.isFile()) {
            yield full;
        }
    }
}

function hashDirectory(dir: string): string {
    const entries: Array<{ path: string; sha256: string }> = [];
    for (const file of walkDir(dir)) {
        const rel = relative(dir, file).replace(/\\/g, '/');
        entries.push({ path: rel, sha256: sha256File(file) });
    }
    entries.sort((a, b) => a.path.localeCompare(b.path));
    const payload = JSON.stringify(entries);
    return createHash('sha256').update(payload).digest('hex');
}

function hashCorpus(config: CorpusConfig): string | null {
    const fullPath = resolve(config.path);
    if (!existsSync(fullPath)) {
        return null;
    }
    const st = statSync(fullPath);
    if (st.isFile()) {
        return sha256File(fullPath);
    }
    if (st.isDirectory()) {
        return hashDirectory(fullPath);
    }
    return null;
}

function main() {
    const { manifest } = parseArgs();
    const manifestData = JSON.parse(readFileSync(manifest, 'utf8'));

    if (!manifestData.corpora) {
        manifestData.corpora = {};
    }

    for (const corpus of CORPORA) {
        manifestData.corpora[corpus.name] = hashCorpus(corpus);
    }

    writeFileSync(manifest, JSON.stringify(manifestData, null, 2) + '\n');
    console.log(`Updated corpora hashes in ${manifest}`);
    console.log(JSON.stringify(manifestData.corpora, null, 2));
}

main();
