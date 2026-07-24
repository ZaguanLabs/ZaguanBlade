#!/usr/bin/env bun
/**
 * Record P0 baseline measurements into the performance manifest.
 *
 * Runs the existing benchmark harnesses (cold index, query latency, incremental
 * reindex) and the size/build reporters, then appends their JSON results to the
 * manifest.  Heavy benchmarks are run in release profile as intended; this
 * script is meant to be invoked once per pinned environment, not on every edit.
 *
 * Usage:
 *   bun scripts/benchmark/record-baseline.ts --manifest=benchmark-results/manifest.json
 */

import { execSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

interface Args {
    manifest: string;
    skipHeavy: boolean;
    corpus?: string;
    conversationFixture?: string;
}

function parseArgs(): Args {
    const manifest =
        process.argv.find((a) => a.startsWith('--manifest='))?.slice('--manifest='.length) ??
        'benchmark-results/manifest.json';
    const skipHeavy = process.argv.includes('--skip-heavy');
    const corpus = process.argv.find((a) => a.startsWith('--corpus='))?.slice('--corpus='.length);
    const conversationFixture = process.argv
        .find((a) => a.startsWith('--conversation-fixture='))
        ?.slice('--conversation-fixture='.length);
    return { manifest, skipHeavy, corpus, conversationFixture };
}

function nowIso(): string {
    return new Date().toISOString();
}

function run(command: string, cwd: string, env?: Record<string, string>): string {
    const mergedEnv = { ...process.env, ...env };
    return execSync(command, {
        cwd,
        encoding: 'utf8',
        stdio: 'pipe',
        env: mergedEnv,
    });
}

function extractBenchJson(output: string): unknown[] {
    const results: unknown[] = [];
    for (const line of output.split('\n')) {
        const sentinel = 'BENCH_JSON ';
        const idx = line.indexOf(sentinel);
        if (idx >= 0) {
            try {
                results.push(JSON.parse(line.slice(idx + sentinel.length)));
            } catch (_error) {
                // Ignore malformed sentinel lines.
            }
        } else if (line.trimStart().startsWith('{') && line.trimEnd().endsWith('}')) {
            // Fallback: any lone JSON object printed by the harness.
            try {
                results.push(JSON.parse(line));
            } catch (_error) {
                // Ignore non-JSON output.
            }
        }
    }
    return results;
}

function loadManifest(path: string): { results: unknown[]; corpora?: Record<string, unknown> } {
    if (existsSync(path)) {
        return JSON.parse(readFileSync(path, 'utf8'));
    }
    return { results: [] };
}

function appendResult(manifest: { results: unknown[] }, type: string, data: Record<string, unknown>) {
    manifest.results.push({
        type,
        recorded_at: nowIso(),
        ...data,
    });
}

async function main() {
    const { manifest, skipHeavy, corpus, conversationFixture } = parseArgs();
    const manifestData = loadManifest(manifest);

    const scriptDir = dirname(fileURLToPath(import.meta.url));
    const repoRoot = resolve(scriptDir, '../..');
    const tauriDir = resolve(repoRoot, 'src-tauri');

    mkdirSync(dirname(manifest), { recursive: true });

    const benchEnv: Record<string, string> = {};
    if (corpus) {
        benchEnv.BENCH_CORPUS = corpus;
    }
    if (conversationFixture) {
        benchEnv.CONVERSATION_FIXTURE = conversationFixture;
    }

    if (!skipHeavy) {
        // Cold index and incremental reindex.
        const coldOutput = run(
            'cargo test --release --test bench_cold_index -- --ignored --nocapture',
            tauriDir,
            benchEnv
        );
        const coldResults = extractBenchJson(coldOutput);
        for (const data of coldResults) {
            appendResult(manifestData, 'cold_index', data as Record<string, unknown>);
        }

        // Query latency (FTS / LIKE).
        const queryOutput = run(
            'cargo test --release --test query_latency -- --ignored --nocapture',
            tauriDir,
            benchEnv
        );
        const queryResults = extractBenchJson(queryOutput);
        for (const data of queryResults) {
            appendResult(manifestData, 'query_latency', data as Record<string, unknown>);
        }

        // Conversation full + paged load.
        const convOutput = run(
            'cargo test --release --test bench_conversation_load -- --ignored --nocapture',
            tauriDir,
            benchEnv
        );
        const convResults = extractBenchJson(convOutput);
        for (const data of convResults) {
            appendResult(manifestData, 'conversation_load', data as Record<string, unknown>);
        }

        // Context-pack assembly and raw git status.
        const miscOutput = run(
            'cargo test --release --test bench_misc -- --ignored --nocapture',
            tauriDir,
            benchEnv
        );
        const miscResults = extractBenchJson(miscOutput);
        for (const data of miscResults) {
            const typed = data as Record<string, unknown>;
            const typeName =
                'command' in typed ? 'git_status' : 'context_pack';
            appendResult(manifestData, typeName, typed);
        }
    }

    // Frontend build size report (fast, no Tauri build). Keep one in-memory
    // manifest owner so a helper cannot append a result that is later
    // overwritten by this process's stale copy.
    run('bun run build', repoRoot);
    const sizeOutput = resolve(repoRoot, 'benchmark-results/size.json');
    run(
        `bun scripts/benchmark/size-report.ts --out=${JSON.stringify(sizeOutput)} --dist=dist`,
        repoRoot,
    );
    const sizeReport = JSON.parse(readFileSync(sizeOutput, 'utf8')) as Record<string, unknown>;
    appendResult(manifestData, 'size', sizeReport);

    // Categories not yet captured by an automated harness.
    appendResult(manifestData, 'startup', { status: 'not_recorded', note: 'requires interactive app launch / startup marks export' });
    appendResult(manifestData, 'idle', { status: 'not_recorded', note: 'requires runtime CPU profiling' });
    appendResult(manifestData, 'chat_stream', { status: 'not_recorded', note: 'requires model inference' });

    writeFileSync(manifest, JSON.stringify(manifestData, null, 2) + '\n');
    console.log(`Appended baseline results to ${manifest}`);
}

main().catch((e) => {
    console.error(e);
    process.exit(1);
});
