import { QueryClient } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useAuthStore } from '@/stores/auth-store';
import * as f from '@/test/fixtures';

import { startQueryPersistence } from './query-persist';

const KEY = 'nexc.query-cache.v1';

describe('query persistence', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    localStorage.clear();
    useAuthStore.getState().setSession(f.authResponse);
  });
  afterEach(() => {
    vi.useRealTimers();
    useAuthStore.getState().clearSession();
  });

  it('saves lists, restores them for the same user and forgets them on sign-out', () => {
    const first = new QueryClient();
    const stop = startQueryPersistence(first);
    first.setQueryData(['issues', 'w', 'list', {}], [{ id: 1 }]);
    first.setQueryData(['graphs', 'detail', 'g'], { big: true });
    first.setQueryData(['settings', 'llm'], { key_hint: '…a1b2' });
    vi.advanceTimersByTime(1_500);
    stop();
    const saved = localStorage.getItem(KEY) ?? '';
    expect(saved).toContain('"issues"');
    expect(saved).not.toContain('key_hint');
    expect(saved).not.toContain('"big"');

    const second = new QueryClient();
    const stopSecond = startQueryPersistence(second);
    expect(second.getQueryData(['issues', 'w', 'list', {}])).toEqual([{ id: 1 }]);

    useAuthStore.getState().clearSession();
    expect(localStorage.getItem(KEY)).toBeNull();
    stopSecond();
  });

  it('does not restore another account’s copy', () => {
    localStorage.setItem(
      KEY,
      JSON.stringify({ userId: 'someone-else', savedAt: Date.now(), state: { queries: [] } }),
    );
    const stop = startQueryPersistence(new QueryClient());
    expect(localStorage.getItem(KEY)).toBeNull();
    stop();
  });
});
