import { describe, expect, it, vi } from 'vitest';

import { retryImport } from './retry-import';

describe('an import tried again', () => {
  it('loads on the second try after the first fails', async () => {
    const load = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(new TypeError('Failed to fetch dynamically imported module'))
      .mockResolvedValueOnce('module');
    await expect(retryImport(load)()).resolves.toBe('module');
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('fails when the second try fails too, and tries no more', async () => {
    const load = vi.fn<() => Promise<string>>().mockRejectedValue(new TypeError('gone'));
    await expect(retryImport(load)()).rejects.toThrow('gone');
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('loads once when the first try works', async () => {
    const load = vi.fn<() => Promise<string>>().mockResolvedValue('module');
    await retryImport(load)();
    expect(load).toHaveBeenCalledTimes(1);
  });
});
