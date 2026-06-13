import assert from 'node:assert/strict';
import test from 'node:test';
import type { IndexHealthSnapshot, IndexHealthStatus } from '../types/blade';
import { formatIndexStatusLabel, shouldShowIndexStatusCue } from './indexHealthStatus';

function makeHealth(overrides: Partial<IndexHealthSnapshot> & { status: IndexHealthStatus }): IndexHealthSnapshot {
    return {
        status: overrides.status,
        indexed_files: overrides.indexed_files ?? 0,
        supported_files: overrides.supported_files ?? 0,
        stale_files: overrides.stale_files ?? 0,
        missing_files: overrides.missing_files ?? 0,
        orphaned_files: overrides.orphaned_files ?? 0,
        queued_files: overrides.queued_files ?? 0,
        active_workers: overrides.active_workers ?? 0,
        symbol_count: overrides.symbol_count ?? 0,
        last_full_scan_ms: overrides.last_full_scan_ms ?? null,
        last_incremental_update_ms: overrides.last_incremental_update_ms ?? null,
        current_file: overrides.current_file,
        message: overrides.message ?? 'Code intelligence status',
    };
}

test('does not show cue for stale or partial symbol index health with pending work', () => {
    assert.equal(shouldShowIndexStatusCue(makeHealth({
        status: 'stale',
        stale_files: 2,
        queued_files: 2,
    })), false);
    assert.equal(shouldShowIndexStatusCue(makeHealth({
        status: 'partial',
        missing_files: 3,
        queued_files: 3,
    })), false);
});

test('does not show cue for fresh or non-pending partial symbol index health', () => {
    assert.equal(shouldShowIndexStatusCue(makeHealth({ status: 'fresh' })), false);
    assert.equal(shouldShowIndexStatusCue(makeHealth({ status: 'partial' })), false);
});

test('shows cue only while checking or actively indexing', () => {
    assert.equal(shouldShowIndexStatusCue(makeHealth({ status: 'checking' })), true);
    assert.equal(shouldShowIndexStatusCue(makeHealth({ status: 'indexing' })), true);
    assert.equal(shouldShowIndexStatusCue(makeHealth({ status: 'error' })), false);
});

test('formats active indexing progress from remaining queued files', () => {
    const health = makeHealth({
        status: 'indexing',
        stale_files: 5,
        queued_files: 3,
        current_file: 'src/main.ts',
    });

    assert.equal(formatIndexStatusLabel(health), 'Indexing src/main.ts (2/5)');
});
