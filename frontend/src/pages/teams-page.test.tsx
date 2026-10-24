import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import { qk } from '@/lib/query-keys';
import type { Team, Workspace } from '@/schemas/workspace';

import TeamsPage from './teams-page';

const { apiSend } = vi.hoisted(() => ({ apiSend: vi.fn() }));

const at = '2026-10-20T09:00:00Z';

const workspace: Workspace = {
  id: '0199dddd-0000-7000-8000-000000000001',
  name: 'Acme',
  slug: 'acme',
  role: 'owner',
  member_count: 3,
  created_at: at,
};

const team: Team = {
  id: '0199dddd-0000-7000-8000-000000000002',
  workspace_id: workspace.id,
  name: 'Engineering',
  key: 'ENG',
  description: '',
  private: true,
  role: 'owner',
  member_count: 1,
  created_at: at,
};

vi.mock('@/lib/api/client', () => ({
  apiRequest: (path: string) => {
    if (path === '/workspaces') return Promise.resolve([workspace]);
    if (path.endsWith('/teams')) return Promise.resolve([team]);
    return Promise.resolve([]);
  },
  apiSend,
  apiDownload: vi.fn(),
}));

describe('deleting a team', () => {
  const listFiles = vi.fn();
  const filesKey = qk.artifacts.list(workspace.id, {});

  /** A file list read earlier and still on screen elsewhere (the Artifacts page, say). */
  function Files() {
    useQuery({ queryKey: filesKey, queryFn: listFiles, staleTime: 60_000 });
    return null;
  }

  it('marks every file list stale, to be read where it is next shown', async () => {
    apiSend.mockResolvedValue(undefined);
    listFiles.mockResolvedValue([]);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <TeamsPage />
          <Files />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    await waitFor(() => expect(listFiles).toHaveBeenCalledTimes(1));

    fireEvent.click(await screen.findByRole('button', { name: 'Delete Engineering' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Delete team' }));
    await waitFor(() => expect(client.getQueryState(filesKey)?.isInvalidated).toBe(true));
    expect(apiSend).toHaveBeenCalledWith(`/workspaces/${workspace.id}/teams/${team.id}`, {
      method: 'DELETE',
    });
    expect(listFiles).toHaveBeenCalledTimes(1);
  });
});
