import { afterEach, describe, expect, it, vi } from 'vitest';

const log = vi.fn((_level: string, _message: string, _detail?: string) => Promise.resolve());
vi.mock('@/services/ipc', () => ({ systemApi: { log: (l: string, m: string, d?: string) => log(l, m, d) } }));

const { describe: describeValue, logToCore } = await import('./log');

afterEach(() => {
  vi.useRealTimers();
  log.mockClear();
});

describe('interface log', () => {
  it('describes errors, command failures and other values', () => {
    expect(describeValue(new TypeError('x is undefined'))).toBe('TypeError: x is undefined');
    expect(describeValue({ kind: 'audio', message: 'No output' })).toBe('audio: No output');
    expect(describeValue('plain')).toBe('plain');
    expect(describeValue({ a: 1 })).toBe('{"a":1}');
    const loop: Record<string, unknown> = {};
    loop.self = loop;
    expect(describeValue(loop)).toBe('[object Object]');
  });

  it('sends at most 30 messages a minute, then says how many it dropped', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    for (let i = 0; i < 35; i++) logToCore('warn', `message ${i}`);
    expect(log).toHaveBeenCalledTimes(30);

    vi.setSystemTime(1_000_000 + 61_000);
    logToCore('error', 'later');
    expect(log).toHaveBeenNthCalledWith(31, 'warn', '5 more interface messages were not logged', undefined);
    expect(log).toHaveBeenNthCalledWith(32, 'error', 'later', undefined);
  });
});
