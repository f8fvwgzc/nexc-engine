import { QueryObserver, type StaleTime } from '@tanstack/react-query';
import { describe, expect, it } from 'vitest';

import { ApiError, ContractError } from '@/lib/api/errors';
import { changesElsewhere, FRESH } from '@/lib/freshness';

import { createQueryClient, shouldRetry } from './query-client';

describe('the query client', () => {
  it('retries what may go away, never an answer about the request', () => {
    const problem = (status: number) => new ApiError({ type: 'about:blank', title: 'x', status });
    expect(shouldRetry(0, problem(503))).toBe(true);
    expect(shouldRetry(1, new TypeError('Failed to fetch'))).toBe(true);
    expect(shouldRetry(2, problem(503))).toBe(false);
    for (const status of [400, 401, 403, 404, 409, 422, 429]) {
      expect(shouldRetry(0, problem(status))).toBe(false);
    }
    expect(shouldRetry(0, new ContractError('/agents', []))).toBe(false);
  });

  it('refetches lists and live state on focus, never setup or catalogues', () => {
    const client = createQueryClient();
    const onFocus = (staleTime: StaleTime) => {
      const queryKey = ['focus', staleTime];
      const observer = new QueryObserver(client, {
        queryKey,
        queryFn: () => 1,
        staleTime,
        enabled: false,
      });
      const stop = observer.subscribe(() => undefined);
      const query = client.getQueryCache().find({ queryKey });
      const refetched = query ? changesElsewhere(query) : undefined;
      stop();
      return refetched;
    };
    expect(onFocus(FRESH.live)).toBe(true);
    expect(onFocus(FRESH.list)).toBe(true);
    expect(onFocus(FRESH.setup)).toBe(false);
    expect(onFocus(FRESH.catalogue)).toBe(false);
    expect(onFocus(Infinity)).toBe(false);
    expect(onFocus('static')).toBe(false);
    expect(client.getDefaultOptions().queries?.refetchOnWindowFocus).toBe(changesElsewhere);
    client.clear();
  });
});
