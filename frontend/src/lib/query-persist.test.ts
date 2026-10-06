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

  it('saves a change made just before the page is left, and refetches what it restores', () => {
    const first = new QueryClient();
    const stop = startQueryPersistence(first);
    first.setQueryData(['workspaces', 'w', 'teams'], [{ id: 'new-team' }]);
    // The page goes away inside the save delay.
    window.dispatchEvent(new Event('pagehide'));
    stop();
    expect(localStorage.getItem(KEY) ?? '').toContain('new-team');

    // However recent the copy is, it only paints first: the restored list is stale.
    const second = new QueryClient({ defaultOptions: { queries: { staleTime: 60_000 } } });
    const stopSecond = startQueryPersistence(second);
    const restored = second.getQueryState(['workspaces', 'w', 'teams']);
    expect(restored?.data).toEqual([{ id: 'new-team' }]);
    expect(restored?.isInvalidated).toBe(true);
    stopSecond();
  });

  it('does not restore a copy written by another build of the app', () => {
    const first = new QueryClient();
    const stop = startQueryPersistence(first);
    first.setQueryData(['issues', 'w', 'list', {}], [{ id: 1 }]);
    vi.advanceTimersByTime(1_500);
    stop();
    const snapshot = JSON.parse(localStorage.getItem(KEY) ?? '{}') as { build: string };
    localStorage.setItem(KEY, JSON.stringify({ ...snapshot, build: 'an-older-build' }));

    const second = new QueryClient();
    const stopSecond = startQueryPersistence(second);
    expect(second.getQueryData(['issues', 'w', 'list', {}])).toBeUndefined();
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
