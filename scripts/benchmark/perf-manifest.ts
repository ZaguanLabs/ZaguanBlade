#!/usr/bin/env bun
/**
 * Performance manifest generator for Zaguán Blade.
 *
 * Emits a JSON file containing the reproducible context for a benchmark run:
 * commit, toolchain versions, OS, CPU, build profile, corpus hashes, and an
 * empty results array ready to be filled by measurement scripts.
 *
 * Usage:
 *   bun scripts/benchmark/perf-manifest.ts --out=benchmark-results/manifest.json
 */

import { execSync } from "node:child_process";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

function run(command: string): string {
    try {
        return execSync(command, { encoding: "utf8", stdio: "pipe" }).trim();
    } catch (_error) {
        return "";
    }
}

function parseArgs(): { out: string; tag?: string } {
    const out = process.argv
        .find((a) => a.startsWith("--out="))
        ?.slice("--out=".length);
    const tag = process.argv
        .find((a) => a.startsWith("--tag="))
        ?.slice("--tag=".length);
    if (!out) {
        console.error("Usage: bun scripts/benchmark/perf-manifest.ts --out=PATH [--tag=LABEL]");
        process.exit(1);
    }
    return { out, tag };
}

function cpuInfo(): { model: string; cores: number } {
    let model = "";
    let cores = 0;
    try {
        const info = execSync("cat /proc/cpuinfo", { encoding: "utf8" });
        const modelMatch = info.match(/model name\s*:\s*(.+)/);
        if (modelMatch) model = modelMatch[1].trim();
        const physicalMatches = info.match(/^physical id\s*:/gm);
        if (physicalMatches) {
            cores = new Set(physicalMatches).size;
        }
    } catch (_error) {
        // Non-Linux hosts may not expose /proc/cpuinfo.
    }
    return { model, cores };
}

async function main() {
    const { out, tag } = parseArgs();
    const { model, cores } = cpuInfo();

    const manifest = {
        version: 1,
        project: "zaguan-blade",
        tag,
        recorded_at: new Date().toISOString(),
        environment: {
            commit: run("git rev-parse HEAD"),
            branch: run("git rev-parse --abbrev-ref HEAD"),
            working_tree_dirty: run("git status --short").length > 0,
            rust_version: run("rustc --version"),
            cargo_version: run("cargo --version"),
            bun_version: run("bun --version"),
            node_version: run("node --version"),
            os: run("uname -a"),
            cpu_model: model,
            cpu_packages: cores,
            total_memory_kb: parseInt(run("grep MemTotal /proc/meminfo | awk '{print $2}'") || "0", 10) || null,
        },
        build_profile: {
            cargo_profile: "release",
            lto: true,
            codegen_units: 1,
            panic: "abort",
            opt_level: 3,
            strip: "symbols",
        },
        corpora: {
            // Hashes to be filled once corpora are pinned.
            tiny: null,
            medium: null,
            large_mixed: null,
            git_heavy: null,
            long_chat: null,
            ui_stress: null,
        },
        results: [] as unknown[],
    };

    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, JSON.stringify(manifest, null, 2) + "\n");
    console.log(`Wrote performance manifest to ${out}`);
}

main().catch((e) => {
    console.error(e);
    process.exit(1);
});
