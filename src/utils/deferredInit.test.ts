import assert from 'node:assert/strict';
import { afterEach, test } from 'node:test';
import { scheduleDeferredInit } from './deferredInit';

type TestGlobal = typeof globalThis & {
    requestIdleCallback?: unknown;
    cancelIdleCallback?: unknown;
};

const testGlobal = globalThis as TestGlobal;
const originalRequestIdleCallback = testGlobal.requestIdleCallback;
const originalCancelIdleCallback = testGlobal.cancelIdleCallback;
const originalConsoleError = console.error;

afterEach(() => {
    Reflect.set(testGlobal, 'requestIdleCallback', originalRequestIdleCallback);
    Reflect.set(testGlobal, 'cancelIdleCallback', originalCancelIdleCallback);
    console.error = originalConsoleError;
});

function wait(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

test('uses requestIdleCallback with the priority timeout when available', async () => {
    let observedTimeout: number | undefined;
    let runIdleCallback: (() => void) | null = null;

    Reflect.set(testGlobal, 'requestIdleCallback', ((
        callback: (deadline: { didTimeout: boolean; timeRemaining: () => number }) => void,
        options?: { timeout?: number },
    ) => {
        observedTimeout = options?.timeout;
        runIdleCallback = () => callback({ didTimeout: false, timeRemaining: () => 8 });
        return 42;
    }));
    Reflect.set(testGlobal, 'cancelIdleCallback', () => undefined);

    let ran = false;
    scheduleDeferredInit(() => {
        ran = true;
    }, { label: 'idle-test', priority: 'background' });

    assert.equal(observedTimeout, 1500);
    assert.equal(ran, false);

    const callbackToRun: unknown = runIdleCallback;
    if (typeof callbackToRun !== 'function') {
        throw new Error('idle callback was not registered');
    }
    callbackToRun();
    await Promise.resolve();
    await Promise.resolve();

    assert.equal(ran, true);
});

test('falls back to setTimeout when requestIdleCallback is unavailable', async () => {
    Reflect.set(testGlobal, 'requestIdleCallback', undefined);
    Reflect.set(testGlobal, 'cancelIdleCallback', undefined);

    let ran = false;
    scheduleDeferredInit(() => {
        ran = true;
    }, { label: 'timeout-test', timeoutMs: 0 });

    await wait(5);

    assert.equal(ran, true);
});

test('cancels scheduled fallback work before it runs', async () => {
    Reflect.set(testGlobal, 'requestIdleCallback', undefined);
    Reflect.set(testGlobal, 'cancelIdleCallback', undefined);

    let ran = false;
    const handle = scheduleDeferredInit(() => {
        ran = true;
    }, { label: 'cancel-test', timeoutMs: 20 });

    handle.cancel();
    await wait(30);

    assert.equal(ran, false);
});

test('does not schedule work when the abort signal is already aborted', async () => {
    const controller = new AbortController();
    controller.abort();

    let ran = false;
    scheduleDeferredInit(() => {
        ran = true;
    }, { label: 'aborted-test', timeoutMs: 0, signal: controller.signal });

    await wait(5);

    assert.equal(ran, false);
});

test('logs task errors without throwing through the scheduler', async () => {
    Reflect.set(testGlobal, 'requestIdleCallback', undefined);
    Reflect.set(testGlobal, 'cancelIdleCallback', undefined);

    let logged = false;
    console.error = (...args: unknown[]) => {
        logged = String(args[0]).includes('[deferredInit] error-test failed:');
    };

    scheduleDeferredInit(() => {
        throw new Error('expected failure');
    }, { label: 'error-test', timeoutMs: 0 });

    await wait(5);

    assert.equal(logged, true);
});
