import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, render, renderHook, screen, waitFor } from '@testing-library/react';
import type { ReactElement, ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { ApiError } from '@/lib/api/errors';
import type { ArtifactFile, ArtifactFolder } from '@/schemas/artifact';

import { useArtifactUpload, type UploadEntry } from './use-artifact-upload';

interface Call {
  method: string;
  path: string;
  query: Record<string, unknown>;
}

/** What the mocked API was asked, and how it answers; each test sets the answer. */
interface Api {
  calls: Call[];
  answer: (call: Call) => Promise<unknown>;
}

const { api, toast } = vi.hoisted(() => {
  const api: Api = { calls: [], answer: () => Promise.resolve(undefined) };
  return { api, toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn() } };
});

vi.mock('@/lib/api/client', () => ({
  apiRequest: (
    path: string,
    _schema: unknown,
    opts: { method?: string; query?: Record<string, unknown> } = {},
  ) => {
    const call = { method: opts.method ?? 'GET', path, query: opts.query ?? {} };
    api.calls.push(call);
    return api.answer(call);
  },
  apiSend: vi.fn(),
  apiDownload: vi.fn(),
}));

vi.mock('sonner', () => ({ toast }));

const at = '2026-10-20T09:00:00Z';
const ws = '0199cccc-0000-7000-8000-000000000001';
const FOLDERS = `/workspaces/${ws}/artifact-folders`;
const FILES = `/workspaces/${ws}/artifact-files`;

const folder = (id: string, name: string, parent: string | null = null): ArtifactFolder => ({
  id,
  workspace_id: ws,
  parent_id: parent,
  name,
  created_at: at,
  updated_at: at,
});

const entry = (path: string): UploadEntry => ({
  file: new File(['x'], path.split('/').pop() ?? path),
  path,
});

/** The server as most tests want it: these folders exist, and every POST succeeds. */
function serve(folders: ArtifactFolder[]) {
  let made = 0;
  api.answer = (call) => {
    if (call.path === FOLDERS && call.method === 'GET') return Promise.resolve(folders);
    if (call.path === FOLDERS) return Promise.resolve(folder(`made-${++made}`, 'made'));
    return Promise.resolve({ id: `file-${String(call.query.name)}` } as ArtifactFile);
  };
}

const uploads = () => api.calls.filter((c) => c.path === FILES && c.method === 'POST');
const folderPosts = () => api.calls.filter((c) => c.path === FOLDERS && c.method === 'POST');

function renderUpload(folderId: string | null = null) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return renderHook(() => useArtifactUpload(ws, { folderId }), { wrapper });
}

beforeEach(() => {
  api.calls = [];
  toast.success.mockReset();
  toast.warning.mockReset();
  toast.error.mockReset();
});

describe('useArtifactUpload', () => {
  it('puts files into a folder of the same name, ignoring case, rather than making one', async () => {
    serve([folder('f-src', 'src')]);
    const { result } = renderUpload();
    act(() => result.current.upload([entry('Src/a.ts')]));
    expect(result.current.busy).toBe(true);
    await waitFor(() => expect(toast.success).toHaveBeenCalledWith('Uploaded 1 file'));
    expect(folderPosts()).toHaveLength(0);
    expect(uploads()).toEqual([
      expect.objectContaining({
        query: expect.objectContaining({ name: 'a.ts', folder_id: 'f-src' }),
      }),
    ]);
    expect(result.current.busy).toBe(false);
    expect(result.current.sending).toBeNull();
  });

  it('skips the files of a folder that could not be made, and says why', async () => {
    serve([]);
    const served = api.answer;
    api.answer = (call) =>
      call.path === FOLDERS && call.method === 'POST'
        ? Promise.reject(
            new ApiError({
              type: 'about:blank',
              title: 'Conflict',
              status: 409,
              detail: 'A folder named docs is already here',
            }),
          )
        : served(call);
    const { result } = renderUpload('f-open');
    act(() => result.current.upload([entry('docs/a.md'), entry('b.ts')]));
    await waitFor(() => expect(toast.warning).toHaveBeenCalled());

    expect(uploads()).toEqual([
      expect.objectContaining({
        query: expect.objectContaining({ name: 'b.ts', folder_id: 'f-open' }),
      }),
    ]);
    const [title, options] = toast.warning.mock.calls[0] as [string, { description: ReactElement }];
    expect(title).toBe('Uploaded 1 file; skipped 2');
    render(options.description);
    expect(
      screen.getByText('docs/: Conflict: A folder named docs is already here'),
    ).toBeInTheDocument();
    expect(screen.getByText('docs/a.md: its folder could not be made')).toBeInTheDocument();
  });

  it('starts a second upload only once the first is done', async () => {
    serve([]);
    const served = api.answer;
    let release = () => {};
    const held = new Promise<void>((resolve) => (release = resolve));
    api.answer = (call) =>
      call.query.name === 'first.md' ? held.then(() => served(call)) : served(call);
    const { result } = renderUpload();
    act(() => {
      result.current.upload([entry('first.md')]);
      result.current.upload([entry('second.md')]);
    });
    await waitFor(() => expect(result.current.sending).toBe('first.md'));
    expect(uploads().map((c) => c.query.name)).toEqual(['first.md']);

    act(() => release());
    await waitFor(() => expect(toast.success).toHaveBeenCalledTimes(2));
    expect(uploads().map((c) => c.query.name)).toEqual(['first.md', 'second.md']);
    expect(result.current.busy).toBe(false);
  });
});
