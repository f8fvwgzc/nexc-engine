import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';

import { qk } from '@/lib/query-keys';

import { useUnread } from './use-unread';

const ws = '0199cccc-0000-7000-8000-000000000001';

const { api } = vi.hoisted(() => ({ api: { paths: [] as string[], count: 3 } }));

vi.mock('@/features/workspaces/use-current-workspace', () => ({
  useCurrentWorkspace: () => ({ current: { id: ws }, all: [] }),
}));

vi.mock('@/lib/api/client', () => ({
  apiRequest: (path: string) => {
    api.paths.push(path);
    return Promise.resolve({ count: api.count });
  },
  apiSend: vi.fn(),
}));

function wrapper(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
}

describe('useUnread', () => {
  it('reads the count, not the inbox, and reads it again when the inbox is set by hand', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useUnread(), { wrapper: wrapper(client) });
    await waitFor(() => expect(result.current).toBe(3));
    expect(api.paths).toEqual([`/workspaces/${ws}/inbox/unread-count`]);

    // Marking read writes the new inbox into the cache: the badge follows.
    api.count = 0;
    await act(async () => {
      client.setQueryData(qk.issues.inbox(ws), []);
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current).toBe(0));
    expect(api.paths).toHaveLength(2);
  });
});
