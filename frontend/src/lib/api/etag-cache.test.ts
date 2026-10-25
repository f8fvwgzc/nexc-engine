import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { z } from 'zod';

import { CACHE_SCHEMA } from '@/lib/query-persist';
import { useAuthStore } from '@/stores/auth-store';
import * as f from '@/test/fixtures';

import { apiRequest } from './client';
import {
  cachedBody,
  conditionalHeaders,
  forgetAll,
  persistable,
  remember,
  startEtagPersistence,
} from './etag-cache';

const KEY = 'nexc.etags.v1';

function jsonResponse(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(status === 304 ? null : JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json', ...headers },
  });
}

beforeEach(() => {
  forgetAll();
  localStorage.clear();
  useAuthStore.getState().setSession(f.authResponse);
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  useAuthStore.getState().clearSession();
  forgetAll();
});

describe('conditional reads', () => {
  it('sends If-None-Match for a URL it has a body for and keeps the body on 304', async () => {
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(jsonResponse({ id: 'a', name: 'first' }, 200, { ETag: '"v1"' }))
      .mockResolvedValueOnce(jsonResponse(null, 304, { ETag: '"v1"' }))
      .mockResolvedValueOnce(jsonResponse({ id: 'a', name: 'second' }, 200, { ETag: '"v2"' }));
    const schema = z.object({ id: z.string(), name: z.string() });

    const first = await apiRequest('/agents/a', schema);
    expect(first.name).toBe('first');
    const plain = fetchMock.mock.calls[0]![1]?.headers as Record<string, string>;
    expect(plain['If-None-Match']).toBeUndefined();
    expect(conditionalHeaders('/api/v1/agents/a')).toEqual({ 'If-None-Match': '"v1"' });

    const again = await apiRequest('/agents/a', schema);
    expect(again.name).toBe('first');
    const conditional = fetchMock.mock.calls[1]![1]?.headers as Record<string, string>;
    expect(conditional['If-None-Match']).toBe('"v1"');

    const changed = await apiRequest('/agents/a', schema);
    expect(changed.name).toBe('second');
    expect(conditionalHeaders('/api/v1/agents/a')).toEqual({ 'If-None-Match': '"v2"' });
  });

  it('asks again plainly when a 304 arrives for a body it no longer keeps', async () => {
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(jsonResponse(null, 304))
      .mockResolvedValueOnce(jsonResponse({ ok: true }));
    expect(await apiRequest('/workspaces', z.object({ ok: z.boolean() }))).toEqual({ ok: true });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('keeps nothing for writes and for answers without a tag', async () => {
    vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(jsonResponse({ id: 'n' }, 200))
      .mockResolvedValueOnce(jsonResponse({ id: 'n' }, 201, { ETag: '"w"' }));
    await apiRequest('/graphs', z.object({ id: z.string() }));
    expect(cachedBody('/api/v1/graphs')).toBeUndefined();
    await apiRequest('/graphs', z.object({ id: z.string() }), { method: 'POST', body: {} });
    expect(cachedBody('/api/v1/graphs')).toBeUndefined();
  });

  it('keeps the persisted lists between visits for the same account and build, not the rest', () => {
    vi.useFakeTimers();
    const stop = startEtagPersistence();
    remember('/api/v1/workspaces', '"w"', [{ id: 'w' }]);
    remember('/api/v1/workspaces/w/issues?limit=50', '"i"', [{ id: 'i' }]);
    remember('/api/v1/settings/llm', '"s"', { key_hint: 'a1b2' });
    remember('/api/v1/agents/a/tokens', '"t"', [{ prefix: 'nexc_agt_a1b2' }]);
    vi.advanceTimersByTime(1_500);
    stop();
    const saved = localStorage.getItem(KEY) ?? '';
    expect(saved).toContain('"w"');
    expect(saved).toContain('"i"');
    expect(saved).not.toContain('key_hint');
    expect(saved).not.toContain('nexc_agt_');
    expect(persistable('/api/v1/agents/a/tokens')).toBe(false);
    expect(persistable('/api/v1/graphs?workspace_id=w')).toBe(true);
    expect(persistable('/api/v1/graphs/g')).toBe(false);
    // Searches and sub-issue lists stay out, as they do of query-persist's copy.
    expect(persistable('/api/v1/workspaces/w/issues?open=true&q=needle')).toBe(false);
    expect(persistable('/api/v1/workspaces/w/issues?parent_id=p')).toBe(false);
    expect(persistable('/api/v1/workspaces/w/issues?team_id=t&open=true')).toBe(true);

    forgetAll();
    localStorage.setItem(KEY, saved);
    const stopSecond = startEtagPersistence();
    expect(conditionalHeaders('/api/v1/workspaces')).toEqual({ 'If-None-Match': '"w"' });
    expect(cachedBody('/api/v1/workspaces/w/issues?limit=50')).toEqual([{ id: 'i' }]);
    expect(cachedBody('/api/v1/settings/llm')).toBeUndefined();

    useAuthStore.getState().clearSession();
    expect(localStorage.getItem(KEY)).toBeNull();
    expect(conditionalHeaders('/api/v1/workspaces')).toEqual({});
    stopSecond();
  });

  it('keeps only the issue lists read last, and no list too long, as query-persist does', () => {
    vi.useFakeTimers();
    const stop = startEtagPersistence();
    remember('/api/v1/workspaces', '"w"', [{ id: 'w' }]);
    for (let n = 1; n <= 10; n++) {
      remember(`/api/v1/workspaces/w/issues?team_id=t${n}`, `"t${n}"`, [{ id: `i${n}` }]);
    }
    remember(
      '/api/v1/graphs?workspace_id=w',
      '"long"',
      Array.from({ length: 501 }, (_, i) => ({ id: `g${i}` })),
    );
    vi.advanceTimersByTime(1_500);
    stop();

    const saved = JSON.parse(localStorage.getItem(KEY) ?? '{}') as { entries: [string][] };
    const urls = saved.entries.map(([url]) => url);
    expect(urls).toEqual([
      '/api/v1/workspaces',
      ...[3, 4, 5, 6, 7, 8, 9, 10].map((n) => `/api/v1/workspaces/w/issues?team_id=t${n}`),
    ]);
  });

  it('ignores a copy written under another cache schema', () => {
    localStorage.setItem(
      KEY,
      JSON.stringify({
        schema: CACHE_SCHEMA - 1,
        userId: f.user.id,
        savedAt: Date.now(),
        entries: [['/api/v1/workspaces', { etag: '"x"', body: [] }]],
      }),
    );
    const stop = startEtagPersistence();
    expect(conditionalHeaders('/api/v1/workspaces')).toEqual({});
    expect(localStorage.getItem(KEY)).toBeNull();
    stop();
  });
});
