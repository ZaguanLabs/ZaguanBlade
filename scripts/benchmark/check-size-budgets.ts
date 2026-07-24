#!/usr/bin/env bun

import { readFileSync } from 'node:fs';

interface SizeReport {
    dist: { total_bytes: number } | null;
    preload: {
        gzip_bytes: number;
        css_gzip_bytes: number;
        assets: Array<{ path: string }>;
    } | null;
}

const reportPath =
    process.argv.find((argument) => argument.startsWith('--report='))?.slice('--report='.length)
    ?? 'benchmark-results/size.json';
const report = JSON.parse(readFileSync(reportPath, 'utf8')) as SizeReport;

const limits = {
    distBytes: 8_500_000,
    initialJsGzipBytes: 200_000,
    initialCssGzipBytes: Math.ceil(16_508 * 1.05),
};

const failures: string[] = [];
if (!report.dist || !report.preload) {
    failures.push('size report is missing dist or initial-graph measurements');
} else {
    if (report.dist.total_bytes > limits.distBytes) {
        failures.push(`dist is ${report.dist.total_bytes} bytes (budget ${limits.distBytes})`);
    }
    if (report.preload.gzip_bytes > limits.initialJsGzipBytes) {
        failures.push(
            `initial JavaScript is ${report.preload.gzip_bytes} gzip bytes `
            + `(budget ${limits.initialJsGzipBytes})`,
        );
    }
    if (report.preload.css_gzip_bytes > limits.initialCssGzipBytes) {
        failures.push(
            `initial CSS is ${report.preload.css_gzip_bytes} gzip bytes `
            + `(budget ${limits.initialCssGzipBytes})`,
        );
    }

    const forbiddenInitialAssets = [
        'pdf',
        'xterm',
        'settingsmodal',
        'qrcode',
        'gitpanel',
        'filehistory',
        'protocolexplorer',
    ];
    const forbidden = report.preload.assets
        .map((asset) => asset.path.toLowerCase())
        .filter((path) => forbiddenInitialAssets.some((name) => path.includes(name)));
    if (forbidden.length > 0) {
        failures.push(`lazy feature code entered the initial graph: ${forbidden.join(', ')}`);
    }
}

if (failures.length > 0) {
    console.error('Size budgets failed:\n- ' + failures.join('\n- '));
    process.exit(1);
}

console.log('Size budgets passed.');
