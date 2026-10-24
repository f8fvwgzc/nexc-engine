import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { ApiError } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { ArtifactFile } from '@/schemas/artifact';
import type { Workspace } from '@/schemas/workspace';

import ArtifactFilePage from './artifact-file-page';

interface Call {
  method: string;
  path: string;
  query: Record<string, unknown>;
  body?: unknown;
}

/** What the mocked API holds and answers. */
interface Server {
  calls: Call[];
  file: ArtifactFile | null;
  /** The text of each version, by number. */
  contents: Record<number, string>;
  /** What a save answers; each test sets it. */
  save: (call: Call) => Promise<unknown>;
  /** Set to make reading the file fail, as it does once someone deletes it. */
  fileError: Error | null;
}

const { server, toast } = vi.hoisted(() => {
  const server: Server = {
    calls: [],
    file: null,
    contents: {},
    save: () => Promise.reject(new Error('no save expected')),
    fileError: null,
  };
  return { server, toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn() } };
});

const at = '2026-10-20T09:00:00Z';
const ids = {
  workspace: '0199cccc-0000-7000-8000-000000000001',
  file: '0199cccc-0000-7000-8000-00000000f001',
};
const FILE = `/artifact-files/${ids.file}`;

const workspace: Workspace = {
  id: ids.workspace,
  name: 'Acme',
  slug: 'acme',
  role: 'member',
  member_count: 3,
  created_at: at,
};

const file = (version: number): ArtifactFile => ({
  id: ids.file,
  workspace_id: ids.workspace,
  folder_id: null,
  name: 'notes.md',
  extension: 'md',
  kind: 'markdown',
  language: 'markdown',
  issue: null,
  current_version: version,
  size: 12,
  sha256: 'ab'.repeat(32),
  duplicated_from: null,
  created_by: null,
  updated_by: null,
  created_at: at,
  updated_at: at,
});

vi.mock('@/lib/api/client', () => ({
  apiRequest: (
    path: string,
    _schema: unknown,
    opts: { method?: string; query?: Record<string, unknown>; body?: unknown } = {},
  ) => {
    const call = { method: opts.method ?? 'GET', path, query: opts.query ?? {}, body: opts.body };
    server.calls.push(call);
    if (path === '/workspaces') return Promise.resolve([workspace]);
    if (path === FILE) {
      return server.fileError ? Promise.reject(server.fileError) : Promise.resolve(server.file);
    }
    if (path === `${FILE}/versions`) return Promise.resolve([]);
    if (path === `${FILE}/content` && call.method === 'PUT') return server.save(call);
    if (path === `${FILE}/content`) {
      const version = Number(call.query.version);
      return Promise.resolve({ file_id: ids.file, version, content: server.contents[version] });
    }
    return Promise.reject(new Error(`unexpected request to ${path}`));
  },
  apiSend: vi.fn(),
  apiDownload: vi.fn(),
}));

vi.mock('sonner', () => ({ toast }));

const conflict = () =>
  new ApiError({ type: 'about:blank', title: 'Conflict', status: 409, detail: 'Newer version' });

/** Opens the file ready to edit, as a new file opens. */
async function openEditor() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createMemoryRouter(
    [{ path: '/app/artifacts/:fileId', element: <ArtifactFilePage /> }],
    { initialEntries: [`/app/artifacts/${ids.file}?edit=1`] },
  );
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  const editor = await screen.findByRole<HTMLTextAreaElement>('textbox', {
    name: 'Content of notes.md',
  });
  return { client, editor };
}

const fileReads = () => server.calls.filter((c) => c.path === FILE).length;

beforeEach(() => {
  server.calls = [];
  server.file = file(3);
  server.contents = { 3: '# Notes\n' };
  server.fileError = null;
  toast.success.mockReset();
  toast.error.mockReset();
});

describe('ArtifactFilePage editor', () => {
  it('says who saved first, keeps the text to copy, and reloads their version', async () => {
    const { editor } = await openEditor();
    fireEvent.change(editor, { target: { value: '# Mine\n' } });
    // Duplicating or deleting would leave the draft behind.
    expect(screen.getByRole('button', { name: 'More actions for notes.md' })).toBeDisabled();
    // Meanwhile someone else saved v4 and v5.
    server.file = file(5);
    server.contents[5] = '# Theirs\n';
    server.save = () => Promise.reject(conflict());
    const readsBefore = fileReads();

    fireEvent.click(screen.getByRole('button', { name: 'Save as v4' }));

    const alert = await screen.findByRole('alert');
    await waitFor(() => expect(alert).toHaveTextContent('Someone else saved v5 first.'));
    expect(fileReads()).toBeGreaterThan(readsBefore);
    expect(screen.getByRole('button', { name: 'Copy my text' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Save as v4' })).toBeDisabled();
    expect(editor).toHaveValue('# Mine\n');

    fireEvent.click(screen.getByRole('button', { name: 'Reload' }));
    await waitFor(() => expect(editor).toHaveValue('# Theirs\n'));
    expect(screen.getByRole('button', { name: 'Save as v6' })).toBeEnabled();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('keeps CRLF line endings when the text is changed', async () => {
    server.contents = { 3: 'a\r\nb\r\n' };
    const sent: string[] = [];
    server.save = async (call) => {
      sent.push(await (call.body as Blob).text());
      return file(4);
    };
    const { editor } = await openEditor();
    expect(editor).toHaveValue('a\nb\n');

    fireEvent.change(editor, { target: { value: 'a\nc\n' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save as v4' }));

    await waitFor(() => expect(toast.success).toHaveBeenCalledWith('Saved v4'));
    expect(sent).toEqual(['a\r\nc\r\n']);
  });

  it('saves the bytes it loaded when nothing was changed, so no version is made', async () => {
    server.contents = { 3: 'a\r\nb\r\n' };
    const sent: string[] = [];
    server.save = async (call) => {
      sent.push(await (call.body as Blob).text());
      return file(3);
    };
    await openEditor();

    fireEvent.click(screen.getByRole('button', { name: 'Save as v4' }));

    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith('Nothing changed, so no new version was saved'),
    );
    expect(sent).toEqual(['a\r\nb\r\n']);
  });

  it('keeps the draft on screen when the file is deleted meanwhile', async () => {
    const { client, editor } = await openEditor();
    fireEvent.change(editor, { target: { value: '# Mine\n' } });
    server.fileError = new ApiError({ type: 'about:blank', title: 'Not Found', status: 404 });

    await act(() => client.invalidateQueries({ queryKey: qk.artifacts.file(ids.file) }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'This file was deleted, or you can no longer see it.',
    );
    expect(editor).toBeInTheDocument();
    expect(editor).toHaveValue('# Mine\n');
    expect(screen.getByRole('button', { name: 'Copy my text' })).toBeEnabled();
  });
});
