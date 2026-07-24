#!/usr/bin/env bun
/**
 * Size reporting script for Zaguán Blade.
 *
 * Measures shipped and packaged artifacts, the frontend distribution, and the
 * initial preload graph.  Writes a JSON report that can be appended to the
 * performance manifest.
 *
 * Usage:
 *   bun scripts/benchmark/size-report.ts --out=benchmark-results/size.json \
 *     [--executable=src-tauri/target/release/zblade] \
 *     [--dist=dist] \
 *     [--manifest=benchmark-results/manifest.json]
 */

import { execFileSync } from "node:child_process";
import {
    copyFileSync,
    mkdirSync,
    mkdtempSync,
    readFileSync,
    readdirSync,
    rmSync,
    statSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { gzipSync } from "node:zlib";

interface SizeEntry {
    path: string;
    bytes: number;
    gzip_bytes?: number;
    description?: string;
}

function gzipBytes(path: string): number | undefined {
    try {
        return gzipSync(readFileSync(path), { level: 9 }).length;
    } catch (_error) {
        return undefined;
    }
}

function fileSize(path: string): number | undefined {
    try {
        return statSync(path).size;
    } catch (_error) {
        return undefined;
    }
}

function walk(dir: string, out: { path: string; bytes: number }[] = []): { path: string; bytes: number }[] {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) {
            walk(full, out);
        } else if (entry.isFile()) {
            out.push({ path: full.replace(process.cwd() + "/", ""), bytes: statSync(full).size });
        }
    }
    return out;
}

function htmlAssets(
    htmlPath: string,
    distDir: string,
    kind: "javascript" | "stylesheet",
): SizeEntry[] {
    try {
        const html = readFileSync(htmlPath, "utf8");
        const references = kind === "javascript"
            ? [
                ...html.matchAll(/<script[^>]*\bsrc="([^"]+)"/g),
                ...html.matchAll(/<link[^>]*\brel="modulepreload"[^>]*\bhref="([^"]+)"/g),
                ...html.matchAll(/<link[^>]*\bhref="([^"]+)"[^>]*\brel="modulepreload"/g),
            ]
            : [
                ...html.matchAll(/<link[^>]*\brel="stylesheet"[^>]*\bhref="([^"]+)"/g),
                ...html.matchAll(/<link[^>]*\bhref="([^"]+)"[^>]*\brel="stylesheet"/g),
            ];
        const entries: SizeEntry[] = [];
        const seen = new Set<string>();
        for (const m of references) {
            const rel = m[1].startsWith("/") ? m[1].slice(1) : m[1];
            if (seen.has(rel)) {
                continue;
            }
            seen.add(rel);
            const path = join(distDir, rel);
            const bytes = fileSize(path);
            if (bytes !== undefined) {
                entries.push({
                    path,
                    bytes,
                    gzip_bytes: gzipBytes(path),
                    description: kind === "javascript" ? "initial JavaScript" : "initial stylesheet",
                });
            }
        }
        return entries;
    } catch (_error) {
        return [];
    }
}

function parseArgs(): { out: string; executable?: string; dist?: string; manifest?: string } {
    const get = (flag: string) => process.argv.find((a) => a.startsWith(flag))?.slice(flag.length);
    const out = get("--out=");
    if (!out) {
        console.error("Usage: bun scripts/benchmark/size-report.ts --out=PATH [--executable=PATH] [--dist=DIR] [--manifest=PATH]");
        process.exit(1);
    }
    return {
        out,
        executable: get("--executable="),
        dist: get("--dist="),
        manifest: get("--manifest="),
    };
}

function main() {
    const { out, executable, dist, manifest } = parseArgs();
    const report: {
        recorded_at: string;
        executable: SizeEntry[];
        dist: { total_bytes: number; largest_files: SizeEntry[]; all_files: SizeEntry[] } | null;
        preload: {
            total_bytes: number;
            gzip_bytes: number;
            assets: SizeEntry[];
            css_bytes: number;
            css_gzip_bytes: number;
            css_assets: SizeEntry[];
        } | null;
    } = {
        recorded_at: new Date().toISOString(),
        executable: [],
        dist: null,
        preload: null,
    };

    if (executable) {
        const bytes = fileSize(executable);
        if (bytes !== undefined) {
            const temporaryDirectory = mkdtempSync(join(tmpdir(), "zblade-size-"));
            const stripped = join(temporaryDirectory, "executable.stripped");
            let strippedBytes: number | undefined;
            let strippedGzipBytes: number | undefined;
            try {
                copyFileSync(executable, stripped);
                execFileSync("strip", ["--strip-unneeded", stripped], { stdio: "pipe" });
                strippedBytes = fileSize(stripped);
                strippedGzipBytes = gzipBytes(stripped);
            } catch (_error) {
                // A stripped comparison is optional on platforms without `strip`.
            } finally {
                rmSync(temporaryDirectory, { recursive: true, force: true });
            }
            report.executable.push({ path: executable, bytes, gzip_bytes: gzipBytes(executable), description: "release executable" });
            if (strippedBytes) {
                report.executable.push({ path: "<stripped>", bytes: strippedBytes, gzip_bytes: strippedGzipBytes, description: "strip --strip-unneeded" });
            }
        }
    }

    if (dist) {
        const all = walk(dist);
        const total = all.reduce((s, f) => s + f.bytes, 0);
        const largest = all
            .map((f) => ({ ...f, gzip_bytes: gzipBytes(f.path) }))
            .sort((a, b) => b.bytes - a.bytes)
            .slice(0, 20);
        report.dist = { total_bytes: total, largest_files: largest, all_files: all };

        const preload = htmlAssets(join(dist, "index.html"), dist, "javascript");
        const stylesheets = htmlAssets(join(dist, "index.html"), dist, "stylesheet");
        const preloadTotal = preload.reduce((s, a) => s + a.bytes, 0);
        const preloadGzip = preload.reduce((s, a) => s + (a.gzip_bytes ?? a.bytes), 0);
        report.preload = {
            total_bytes: preloadTotal,
            gzip_bytes: preloadGzip,
            assets: preload,
            css_bytes: stylesheets.reduce((sum, asset) => sum + asset.bytes, 0),
            css_gzip_bytes: stylesheets.reduce(
                (sum, asset) => sum + (asset.gzip_bytes ?? asset.bytes),
                0,
            ),
            css_assets: stylesheets,
        };
    }

    if (manifest) {
        try {
            const m = JSON.parse(readFileSync(manifest, "utf8"));
            m.results = m.results || [];
            m.results.push({ type: "size", ...report });
            writeFileSync(manifest, JSON.stringify(m, null, 2) + "\n");
        } catch (_error) {
            // manifest update is optional
        }
    }

    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, JSON.stringify(report, null, 2) + "\n");
    console.log(`Wrote size report to ${out}`);
}

main();
