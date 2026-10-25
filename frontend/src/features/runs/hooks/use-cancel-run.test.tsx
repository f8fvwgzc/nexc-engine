import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';

import { useStartRun } from '@/features/graph/hooks/use-graph-mutations';
import { qk } from '@/lib/query-keys';
import { run } from '@/test/fixtures';

import { useCancelRun } from './use-cancel-run';

vi.mock('@/lib/api/client', () => ({
  apiRequest: (path: string) =>
    Promise.resolve(path.endsWith('/cancel') ? { ...run, status: 'cancelled' } : run),
  apiSend: vi.fn(),
}));

const ws = '0199cccc-0000-7000-8000-000000000001';

/** A client holding the Runs page's list, read moments ago, and a wrapper giving it to hooks. */
function setup() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(qk.runs.workspace(ws), [{ ...run, graph_name: 'Tender research' }]);
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const stale = () => client.getQueryState(qk.runs.workspace(ws))?.isInvalidated;
  return { wrapper, stale };
}

describe('the Runs page list', () => {
  it('is read again once a run starts on a canvas', async () => {
    const { wrapper, stale } = setup();
    const { result } = renderHook(() => useStartRun(run.graph_id), { wrapper });
    expect(stale()).toBe(false);

    act(() => result.current.mutate({ force: false }));

    await waitFor(() => expect(stale()).toBe(true));
  });

  it('is read again once a run is cancelled', async () => {
    const { wrapper, stale } = setup();
    const { result } = renderHook(() => useCancelRun(), { wrapper });

    act(() => result.current.mutate(run.id));

    await waitFor(() => expect(stale()).toBe(true));
  });
});
