import { beforeEach, describe, expect, it, vi } from 'vitest';
import { z } from 'zod';

import { useAuthStore } from '@/stores/auth-store';
import * as f from '@/test/fixtures';

import { apiRequest } from './client';
import { ApiError } from './errors';
import { refreshSession } from './session';

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': status >= 400 ? 'application/problem+json' : 'application/json' },
  });
}

beforeEach(() => {
  useAuthStore.setState({ status: 'booting', user: null, accessToken: null, expiresAt: null });
});

describe('auth store', () => {
  it('keeps the session in memory only', () => {
    const setItem = vi.spyOn(Storage.prototype, 'setItem');
    useAuthStore.getState().setSession(f.authResponse);
    const state = useAuthStore.getState();
    expect(state.status).toBe('authenticated');
    expect(state.accessToken).toBe(f.authResponse.access_token);
    expect(state.expiresAt).toBeGreaterThan(Date.now());
    expect(setItem).not.toHaveBeenCalled();
    state.clearSession();
    expect(useAuthStore.getState()).toMatchObject({
      status: 'anonymous',
      accessToken: null,
      user: null,
    });
  });
});

describe('refreshSession', () => {
  it('is single-flight: concurrent callers share one POST /auth/refresh', async () => {
    let resolve!: (r: Response) => void;
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockImplementation(() => new Promise<Response>((r) => (resolve = r)));

    const calls = [refreshSession(), refreshSession(), refreshSession()];
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe('/api/v1/auth/refresh');
    expect(init).toMatchObject({ method: 'POST', credentials: 'include' });
    expect((init?.headers as Record<string, string>)['X-Requested-With']).toBe('nexc');

    resolve(jsonResponse(f.authResponse));
    const results = await Promise.all(calls);
    expect(results.every((r) => r?.access_token === f.authResponse.access_token)).toBe(true);
    expect(useAuthStore.getState().status).toBe('authenticated');

    // A later call starts a new request once the previous one settled.
    fetchMock.mockResolvedValueOnce(jsonResponse(f.authResponse));
    await refreshSession();
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('clears the session when the refresh cookie is rejected', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse({ title: 'Unauthorized', status: 401 }, 401),
    );
    expect(await refreshSession()).toBeNull();
    expect(useAuthStore.getState().status).toBe('anonymous');
  });
});

describe('apiRequest', () => {
  it('refreshes once on 401 and retries with the new token', async () => {
    useAuthStore.getState().setSession({ ...f.authResponse, access_token: 'expired' });
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(jsonResponse({ title: 'Unauthorized', status: 401 }, 401))
      .mockResolvedValueOnce(jsonResponse({ ...f.authResponse, access_token: 'fresh' }))
      .mockResolvedValueOnce(jsonResponse(f.user));

    const me = await apiRequest('/auth/me', z.object({ id: z.string() }));
    expect(me.id).toBe(f.user.id);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    const retryHeaders = fetchMock.mock.calls[2]![1]?.headers as Record<string, string>;
    expect(retryHeaders.Authorization).toBe('Bearer fresh');
  });

  it('surfaces problem+json as ApiError with field errors', async () => {
    useAuthStore.getState().setSession(f.authResponse);
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse(
        { title: 'Validation failed', status: 422, errors: { name: ['too long'] } },
        422,
      ),
    );
    const error = await apiRequest('/graphs', z.unknown(), { method: 'POST', body: {} }).catch(
      (e: unknown) => e,
    );
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).fieldErrors).toEqual({ name: ['too long'] });
  });
});
