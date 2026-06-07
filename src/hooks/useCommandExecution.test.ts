import assert from 'node:assert/strict';
import test from 'node:test';
import { buildRunCommandTerminalSpawnCommand } from './useCommandExecution';

test('buildRunCommandTerminalSpawnCommand keeps blocking commands sentinel-free', () => {
    const command = buildRunCommandTerminalSpawnCommand('call-1', 'pnpm add -D prettier@latest', true);

    assert.equal(command, 'pnpm add -D prettier@latest');
    assert.doesNotMatch(command, /##BLADE_CMD_/);
    assert.doesNotMatch(command, /printf '\\n/);
});

test('buildRunCommandTerminalSpawnCommand keeps non-blocking commands sentinel-free', () => {
    const command = buildRunCommandTerminalSpawnCommand('call-2', 'bun run dev', false, 250);

    assert.equal(command, 'bun run dev');
    assert.doesNotMatch(command, /##BLADE_CMD_/);
    assert.doesNotMatch(command, /\[run_command\] detached pid=/);
});
