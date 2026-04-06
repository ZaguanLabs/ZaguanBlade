import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { stripAllAnsi } from './CommandOutputDisplay';

describe('stripAllAnsi', () => {
    it('preserves bracketed dynamic route segments in command output', () => {
        const input = [
            'Import trace:',
            '  Server Component:',
            '    ./src/components/dashboard-shell.tsx',
            '    ./src/app/(app)/dashboard/[businessSlug]/page.tsx',
            '    at ignore-listed frames',
            ' ELIFECYCLE  Command failed with exit code 1.',
        ].join('\n');

        assert.equal(stripAllAnsi(input), input);
    });

    it('strips orphaned ANSI fragments while keeping plain text intact', () => {
        assert.equal(stripAllAnsi('Error: [38;5;196mboom[0m'), 'Error: boom');
        assert.equal(stripAllAnsi('route [slug] is valid'), 'route [slug] is valid');
    });
});
