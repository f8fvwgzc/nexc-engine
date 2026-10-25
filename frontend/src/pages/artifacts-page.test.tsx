import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ArtifactFile, ArtifactFolder } from '@/schemas/artifact';
import type { IssueSummary } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import ArtifactsPage from './artifacts-page';

interface Call {
  method: string;
  path: string;
  query: Record<string, unknown>;
}

const { server } = vi.hoisted(() => ({
  server: {
    calls: [] as Call[],
    /** Answers a call before the defaults when it returns something. */
    answer: undefined as ((call: Call) => unknown) | undefined,
  },
}));

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
    const call = { method: opts.method ?? 'GET', path, query: opts.query ?? {} };
    server.calls.push(call);
    const answer = server.answer?.(call);
    if (answer !== undefined) return Promise.resolve(answer);
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

beforeAll(() => {
  // jsdom has no layout; the issue filter's menu (cmdk) scrolls the chosen item into view.
  if (!('scrollIntoView' in Element.prototype)) {
    Object.defineProperty(Element.prototype, 'scrollIntoView', { value: vi.fn() });
  }
});

beforeEach(() => {
  server.calls = [];
  server.answer = undefined;
});

const file = (n: number): ArtifactFile => ({
  id: `0199cccc-0000-7000-8000-${String(n).padStart(12, '0')}`,
  workspace_id: ws,
  folder_id: null,
  name: `notes-${n}.md`,
  extension: 'md',
  kind: 'markdown',
  language: 'markdown',
  issue: null,
  current_version: 1,
  size: 7,
  sha256: 'x',
  duplicated_from: null,
  created_by: null,
  updated_by: null,
  created_at: at,
  updated_at: `2026-10-20T09:00:${String(59 - (n % 60)).padStart(2, '0')}Z`,
});

const issue = (n: number, title: string): Partial<IssueSummary> => ({
  id: `0199cccc-0000-7000-8000-0000000001${String(n).padStart(2, '0')}`,
  identifier: `ENG-${n}`,
  title,
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

  // Two pages of 200 rows take a while to render in jsdom on a busy machine.
  it('lists every file a page at a time when searching', { timeout: 20_000 }, async () => {
    const pages = [Array.from({ length: 200 }, (_, i) => file(i)), [file(200)]];
    // Only the list of every file asks for a page (`limit`); a folder's comes whole.
    server.answer = ({ path, query }) => {
      if (!path.endsWith('/artifact-files') || query.limit === undefined) return undefined;
      return query.before ? pages[1] : pages[0];
    };
    renderPage();
    const box = await screen.findByRole('textbox', { name: 'Search files' });
    expect(fileLists()[0]?.query.root).toBe('true');
    expect(fileLists()[0]?.query.limit).toBeUndefined();
    fireEvent.change(box, { target: { value: 'notes' } });
    expect(await screen.findByText('notes-199.md')).toBeInTheDocument();
    const page = fileLists().find((c) => c.query.q === 'notes');
    expect(page?.query).toMatchObject({ limit: 200, before: undefined });

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('notes-200.md')).toBeInTheDocument();
    const last = pages[0]![199]!;
    expect(fileLists().at(-1)?.query).toMatchObject({
      q: 'notes',
      before: `${last.updated_at},${last.id}`,
    });
    expect(screen.queryByRole('button', { name: 'Load more' })).toBeNull();
  });

  it('looks issues up as they are typed instead of reading every issue', async () => {
    server.answer = ({ path, query }) => {
      if (!path.endsWith('/issues')) return undefined;
      return query.q ? [issue(7, 'Rocket budget')] : [issue(1, 'Recent work')];
    };
    const router = renderPage();
    await screen.findByRole('heading', { name: 'Artifacts' });
    const issueLists = () => server.calls.filter((c) => c.path.endsWith('/issues'));
    expect(issueLists()).toEqual([]);

    fireEvent.click(screen.getByRole('combobox', { name: 'Issue' }));
    expect(await screen.findByText('Recent work')).toBeInTheDocument();
    expect(issueLists()[0]?.query).toMatchObject({ limit: 20 });
    fireEvent.change(screen.getByPlaceholderText('Find an issue…'), {
      target: { value: 'rocket' },
    });
    fireEvent.click(await screen.findByText('Rocket budget'));

    expect(issueLists().at(-1)?.query).toMatchObject({ q: 'rocket', limit: 20 });
    expect(router.state.location.search).toBe(`?issue=${issue(7, '').id}`);
  });
});
