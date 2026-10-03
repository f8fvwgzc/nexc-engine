import { describe, expect, it } from 'vitest';

import { backoffDelay } from './backoff';
import { keyedThrottle } from './throttle';

describe('backoffDelay', () => {
  it('grows exponentially and caps at maxMs', () => {
    const mid = () => 0.5; // no jitter offset
    expect(backoffDelay(0, { baseMs: 100 }, mid)).toBe(100);
    expect(backoffDelay(3, { baseMs: 100 }, mid)).toBe(800);
    expect(backoffDelay(20, { baseMs: 100, maxMs: 5000 }, mid)).toBe(5000);
  });

  it('applies bounded jitter', () => {
    expect(backoffDelay(2, { baseMs: 100, jitter: 0.5 }, () => 0)).toBe(200);
    expect(backoffDelay(2, { baseMs: 100, jitter: 0.5 }, () => 1)).toBe(600);
  });
});

describe('keyedThrottle', () => {
  it('sends the first call immediately and only the latest trailing call per key', () => {
    vi.useFakeTimers();
    const calls: [string, number][] = [];
    const t = keyedThrottle(100, (key, x: number) => calls.push([key, x]));
    t.call('a', 1);
    t.call('a', 2);
    t.call('a', 3);
    t.call('b', 9);
    expect(calls).toEqual([
      ['a', 1],
      ['b', 9],
    ]);
    vi.advanceTimersByTime(100);
    expect(calls).toEqual([
      ['a', 1],
      ['b', 9],
      ['a', 3],
    ]);
    vi.useRealTimers();
  });
});
