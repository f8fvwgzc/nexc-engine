import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { ApiError } from '@/lib/api/errors';
import type { ArtifactFile, RunArtifact } from '@/schemas/artifact';
import type { Issue } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import { IssueArtifacts } from './issue-artifacts';

const { api, toast } = vi.hoisted(() => ({
  api: {
    /** The issue's files, asked for on every read. */
    files: (): Promise<ArtifactFile[]> => Promise.resolve([]),
    produced: [] as RunArtifact[],
    /** What a new file or an upload answers, by its query. */
    create: (_query: Record<string, unknown>): Promise<unknown> =>
      Promise.reject(new Error('no file expected')),
  },
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn() },
}));

// The API answers by path: the issue's files, the files its runs produced, and a new file.
vi.mock('@/lib/api/client', () => ({
  apiRequest: (
    path: string,
    _schema: unknown,
    opts: { method?: string; query?: Record<string, unknown> } = {},
  ) => {
    if (opts.method === 'POST') return api.create(opts.query ?? {});
    if (path.endsWith('/run-artifacts')) return Promise.resolve(api.produced);
    return api.files();
  },
  apiSend: vi.fn(),
  apiDownload: vi.fn(),
}));

vi.mock('sonner', () => ({ toast }));

const at = '2026-10-20T09:00:00Z';
const ids = {
  workspace: '0199cccc-0000-7000-8000-000000000001',
  issue: '0199cccc-0000-7000-8000-000000000002',
  graph: '0199cccc-0000-7000-8000-000000000003',
  run: '0199cccc-0000-7000-8000-000000000004',
  node: '0199cccc-0000-7000-8000-000000000005',
};

const workspace = (role: Workspace['role'] = 'member'): Workspace => ({
  id: ids.workspace,
  name: 'Acme',
  slug: 'acme',
  role,
  member_count: 3,
  created_at: at,
});

const issue = {
  id: ids.issue,
  identifier: 'ENG-12',
  title: 'Price the pilot',
  graph_id: ids.graph,
} as Issue;

const file: ArtifactFile = {
  id: '0199cccc-0000-7000-8000-00000000f001',
  workspace_id: ids.workspace,
  folder_id: null,
  name: 'pricing.ts',
  extension: 'ts',
  kind: 'code',
  language: 'typescript',
  issue: { id: ids.issue, identifier: 'ENG-12', title: 'Price the pilot' },
  current_version: 4,
  size: 812,
  sha256: 'ab'.repeat(32),
  duplicated_from: null,
  created_by: null,
  updated_by: null,
  created_at: at,
  updated_at: at,
};

const produced = (n: number, path: string): RunArtifact => ({
  id: `0199cccc-0000-7000-8000-${String(n).padStart(12, '0')}`,
  run_id: ids.run,
  node_id: ids.node,
  path,
  size: 100,
  mime: 'text/plain',
  created_at: at,
  quarantined_at: null,
  quarantine_reason: '',
  released_at: null,
  run_status: 'succeeded',
  run_created_at: at,
});

const ISSUE_PAGE = `/app/issues/${ids.issue}`;

function renderSection(role?: Workspace['role']) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createMemoryRouter(
    [
      { path: ISSUE_PAGE, element: <IssueArtifacts workspace={workspace(role)} issue={issue} /> },
      { path: '/app/artifacts/:fileId', element: <p>The file’s page</p> },
    ],
    { initialEntries: [ISSUE_PAGE] },
  );
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

/** The server refusing a name the issue already has. */
const taken = () =>
  new ApiError({
    type: 'about:blank',
    title: 'Conflict',
    status: 409,
    detail: 'ENG-12 already has a file named notes.md',
  });

beforeEach(() => {
  api.files = () => Promise.resolve([]);
  api.produced = [];
  api.create = () => Promise.reject(new Error('no file expected'));
  toast.success.mockReset();
  toast.warning.mockReset();
  toast.error.mockReset();
});

describe('IssueArtifacts', () => {
  it('keeps its heading, Upload and New file while the files load', async () => {
    api.files = () => new Promise(() => {});
    renderSection();
    expect(await screen.findByRole('status', { name: 'Loading the files' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Artifacts' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Upload/ })).toBeEnabled();
    expect(screen.getByRole('button', { name: /New file/ })).toBeEnabled();
  });

  it('says the files could not be read, and reads them again on Try again', async () => {
    api.files = () =>
      Promise.reject(
        new ApiError({ type: 'about:blank', title: 'Internal Server Error', status: 500 }),
      );
    renderSection();
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Couldn’t load the files.');
    expect(screen.queryByText(/No files yet/)).toBeNull();
    expect(screen.getByRole('button', { name: /Upload/ })).toBeEnabled();
    expect(screen.getByRole('button', { name: /New file/ })).toBeEnabled();

    api.files = () => Promise.resolve([file]);
    fireEvent.click(within(alert).getByRole('button', { name: 'Try again' }));

    expect(await screen.findByRole('link', { name: 'pricing.ts' })).toBeInTheDocument();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('lists the files, each opening its own page, with its version', async () => {
    api.files = () => Promise.resolve([file]);
    renderSection();
    const link = await screen.findByRole('link', { name: 'pricing.ts' });
    expect(link).toHaveAttribute('href', `/app/artifacts/${file.id}`);
    expect(screen.getByText('v4')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Upload/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /New file/ })).toBeInTheDocument();
  });

  it('offers to keep only the text files the runs produced', async () => {
    api.produced = [produced(1, 'reports/summary.md'), produced(2, 'reports/deck.pptx')];
    renderSection();
    expect(await screen.findByText('From runs')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Keep summary.md with the issue' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Keep deck.pptx with the issue' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Download reports/deck.pptx' })).toBeEnabled();
  });

  it('lets a guest read and download, not change', async () => {
    api.files = () => Promise.resolve([file]);
    renderSection('guest');
    await screen.findByRole('link', { name: 'pricing.ts' });
    expect(screen.queryByRole('button', { name: /Upload/ })).toBeNull();
    expect(screen.queryByRole('button', { name: /New file/ })).toBeNull();
  });

  it('shows why a new file’s name was refused, and makes it once the name is free', async () => {
    const router = renderSection();
    fireEvent.click(await screen.findByRole('button', { name: /New file/ }));
    const dialog = await screen.findByRole('dialog', { name: 'New file' });
    const field = within(dialog).getByRole('textbox', { name: 'File name' });
    api.create = () => Promise.reject(taken());

    fireEvent.change(field, { target: { value: 'notes.md' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Make file' }));

    expect(await within(dialog).findByRole('alert')).toHaveTextContent(
      'Conflict: ENG-12 already has a file named notes.md',
    );
    expect(router.state.location.pathname).toBe(ISSUE_PAGE);
    expect(toast.error).not.toHaveBeenCalled();

    // Another name: the refusal goes, and the file is made and opened.
    fireEvent.change(field, { target: { value: 'notes-2.md' } });
    expect(within(dialog).queryByRole('alert')).toBeNull();
    api.create = (query) => Promise.resolve({ ...file, id: 'f2', name: String(query.name) });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Make file' }));

    await waitFor(() => expect(router.state.location.pathname).toBe('/app/artifacts/f2'));
    expect(router.state.location.search).toBe('?edit=1');
  });

  it('lists an upload the issue already has a file of that name for, and is ready again', async () => {
    api.create = () => Promise.reject(taken());
    renderSection();
    await screen.findByText(/No files yet/);

    fireEvent.change(screen.getByLabelText('Files to attach to ENG-12'), {
      target: { files: [new File(['# Notes'], 'notes.md')] },
    });

    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    const [title, options] = toast.error.mock.calls[0] as [string, { description: ReactNode }];
    expect(title).toBe('Nothing uploaded: 1 file skipped');
    render(<>{options.description}</>);
    expect(
      screen.getByText('notes.md: Conflict: ENG-12 already has a file named notes.md'),
    ).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole('button', { name: 'Upload' })).toBeEnabled());
  });
});
