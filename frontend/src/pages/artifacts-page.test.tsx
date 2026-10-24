import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ArtifactFolder } from '@/schemas/artifact';
import type { Workspace } from '@/schemas/workspace';

import ArtifactsPage from './artifacts-page';

interface Call {
  method: string;
  path: string;
  query: Record<string, unknown>;
}

const { server } = vi.hoisted(() => ({ server: { calls: [] as Call[] } }));

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

const docs: ArtifactFolder = {
  id: '0199cccc-0000-7000-8000-0000000000d1',
  workspace_id: ws,
  parent_id: null,
  name: 'docs',
  created_at: at,
  updated_at: at,
};

vi.mock('@/lib/api/client', () => ({
  apiRequest: (
    path: string,
    _schema: unknown,
    opts: { method?: string; query?: Record<string, unknown> } = {},
  ) => {
    server.calls.push({ method: opts.method ?? 'GET', path, query: opts.query ?? {} });
    if (path === '/workspaces') return Promise.resolve([workspace]);
    if (path.endsWith('/artifact-folders')) return Promise.resolve([docs]);
    return Promise.resolve([]);
  },
  apiSend: vi.fn(),
  apiDownload: vi.fn(),
}));

function renderPage(at = '/app/artifacts') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createMemoryRouter([{ path: '/app/artifacts', element: <ArtifactsPage /> }], {
    initialEntries: [at],
  });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

const fileLists = () => server.calls.filter((c) => c.path.endsWith('/artifact-files'));
const uploads = () => server.calls.filter((c) => c.method === 'POST');

/** Drags a file from the desktop over the page and drops it there. */
function dropFile(on: HTMLElement) {
  const dataTransfer = {
    types: ['Files'],
    items: [],
    files: [new File(['# Notes'], 'notes.md')],
    dropEffect: 'none',
  };
  fireEvent.dragOver(on, { dataTransfer });
  const overlay = screen.queryByText(/^Drop to upload into/);
  fireEvent.drop(on, { dataTransfer });
  return overlay;
}

beforeEach(() => {
  server.calls = [];
});

describe('ArtifactsPage', () => {
  it('leaves the search when a folder in the results is opened', async () => {
    const router = renderPage();
    const box = await screen.findByRole('textbox', { name: 'Search files' });
    fireEvent.change(box, { target: { value: 'doc' } });
    await waitFor(() => expect(fileLists().some((c) => c.query.q === 'doc')).toBe(true));
    const trail = screen.getByRole('navigation', { name: 'Folders' });
    expect(within(trail).getByText('Search results')).toBeInTheDocument();

    fireEvent.click(await screen.findByRole('link', { name: 'docs' }));

    expect(router.state.location.search).toBe(`?folder=${docs.id}`);
    expect(box).toHaveValue('');
    await waitFor(() =>
      expect(within(trail).getByText('docs')).toHaveAttribute('aria-current', 'page'),
    );
    expect(within(trail).queryByText('Search results')).toBeNull();
    await waitFor(() => expect(fileLists().some((c) => c.query.folder_id === docs.id)).toBe(true));
  });

  it('leaves the search for All files even when already at the top', async () => {
    renderPage();
    const box = await screen.findByRole('textbox', { name: 'Search files' });
    fireEvent.change(box, { target: { value: 'notes' } });
    const trail = screen.getByRole('navigation', { name: 'Folders' });
    await waitFor(() => expect(within(trail).getByText('Search results')).toBeInTheDocument());

    fireEvent.click(within(trail).getByRole('link', { name: 'All files' }));

    expect(box).toHaveValue('');
    expect(within(trail).queryByText('Search results')).toBeNull();
  });

  it('uploads a file dropped on a folder’s page into that folder', async () => {
    renderPage(`/app/artifacts?folder=${docs.id}`);
    const header = await screen.findByRole('heading', { name: 'Artifacts' });
    await waitFor(() => expect(screen.getByRole('button', { name: /New file/ })).toBeEnabled());

    expect(dropFile(header)).toHaveTextContent('Drop to upload into docs');

    await waitFor(() => expect(uploads()).toHaveLength(1));
    expect(uploads()[0]?.query).toMatchObject({ name: 'notes.md', folder_id: docs.id });
  });

  it('takes no drop on the page of a folder that is not there', async () => {
    renderPage('/app/artifacts?folder=0199cccc-0000-7000-8000-0000000000ff');
    expect(await screen.findByText('This folder is not here')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Upload files/ })).toBeDisabled();

    expect(dropFile(screen.getByRole('heading', { name: 'Artifacts' }))).toBeNull();

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(uploads()).toEqual([]);
  });
});
