import assert from 'node:assert/strict';
import test from 'node:test';
import { buildRunCommandTerminalSpawnCommand } from './useCommandExecution';

test('buildRunCommandTerminalSpawnCommand wraps blocking commands with command exit sentinels', () => {
    const command = buildRunCommandTerminalSpawnCommand('call-1', 'pnpm add -D prettier@latest', true);

    assert.match(command, /##BLADE_CMD_START:%s##/);
    assert.match(command, /##BLADE_CMD_EXIT:%s:%s##/);
    assert.match(command, /__blade_ec=\$\?/);
    assert.match(command, /exit "\$__blade_ec"/);
});

test('buildRunCommandTerminalSpawnCommand wraps non-blocking commands with detached success sentinel', () => {
    const command = buildRunCommandTerminalSpawnCommand('call-2', 'bun run dev', false, 250);

    assert.match(command, /##BLADE_CMD_START:%s##/);
    assert.match(command, /##BLADE_CMD_EXIT:%s:%s##/);
    assert.match(command, /\[run_command\] detached pid=/);
    assert.match(command, /__blade_ec=0/);
    assert.match(command, /sleep 0\.25/);
});
