import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { HelmetProvider } from 'react-helmet-async';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import { run } from '@/test/fixtures';
import type { WorkspaceRun } from '@/schemas/run';
import type { Workspace } from '@/schemas/workspace';

import RunsPage from './runs-page';

const at = '2026-10-20T09:00:00Z';
const ws = '0199cccc-0000-7000-8000-000000000001';

const workspace: Workspace = {
  id: ws,
  name: 'Acme',
  slug: 'acme',
  role: 'member',
  member_count: 3,
  created_at: at,
};

const runs: WorkspaceRun[] = [
  { ...run, graph_name: 'Tender research' },
  { ...run, id: '0190a1b2-c3d4-7e5f-8a9b-6c1d2e3f4a5c', graph_name: 'Pricing model' },
];

const { paths } = vi.hoisted(() => ({ paths: [] as string[] }));

vi.mock('@/lib/api/client', () => ({
  apiRequest: (path: string) => {
    paths.push(path);
    if (path === '/workspaces') return Promise.resolve([workspace]);
    if (path === `/workspaces/${ws}/runs`) return Promise.resolve(runs);
    return Promise.resolve([]);
  },
  apiSend: vi.fn(),
}));

describe('RunsPage', () => {
  it('lists the workspace’s runs from one request, with their graphs’ names', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const router = createMemoryRouter([{ path: '/app/runs', element: <RunsPage /> }], {
      initialEntries: ['/app/runs'],
    });
    render(
      <HelmetProvider>
        <QueryClientProvider client={client}>
          <RouterProvider router={router} />
        </QueryClientProvider>
      </HelmetProvider>,
    );

    expect(await screen.findByRole('link', { name: 'Tender research' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Pricing model' })).toBeInTheDocument();
    // No graph list and no request per graph.
    expect(paths.filter((p) => p !== '/workspaces')).toEqual([`/workspaces/${ws}/runs`]);
  });
});
