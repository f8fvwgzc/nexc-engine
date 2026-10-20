import { describe, expect, it } from 'vitest';

import { renewalDelay } from './api';

const minted = Date.parse('2026-10-14T12:00:00Z');
const after = (seconds: number) => new Date(minted + seconds * 1000).toISOString();

describe('renewing the embedded UI’s session', () => {
  it('renews once three quarters of the session’s own life are spent', () => {
    expect(renewalDelay(after(3600), minted)).toBe(45 * 60_000);
    expect(renewalDelay(after(600), minted)).toBe(450_000);
    expect(renewalDelay(after(60), minted)).toBe(45_000);
  });

  it('never renews in a loop, and copes with what it cannot read', () => {
    expect(renewalDelay(after(-30), minted)).toBe(10_000);
    expect(renewalDelay(after(5), minted)).toBe(10_000);
    expect(renewalDelay('not a date', minted)).toBe(10_000);
    expect(renewalDelay(undefined, minted)).toBe(10_000);
    expect(renewalDelay('2099-01-01T00:00:00Z', minted)).toBe(2 ** 31 - 1);
  });
});
