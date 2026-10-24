import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ArtifactFile, RunArtifact } from '@/schemas/artifact';
import type { Issue } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import { IssueArtifacts } from './issue-artifacts';

const { api } = vi.hoisted(() => ({
  api: {
    files: null as Promise<ArtifactFile[]> | null,
    produced: [] as RunArtifact[],
  },
}));

// The API answers by path: the issue's files, and the files its runs produced.
vi.mock('@/lib/api/client', () => ({
  apiRequest: (path: string) =>
    path.endsWith('/run-artifacts') ? Promise.resolve(api.produced) : api.files,
  apiSend: vi.fn(),
  apiDownload: vi.fn(),
}));

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

function renderSection(role?: Workspace['role']) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <IssueArtifacts workspace={workspace(role)} issue={issue} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  api.files = Promise.resolve([]);
  api.produced = [];
});

describe('IssueArtifacts', () => {
  it('draws nothing until the issue’s files arrive', () => {
    api.files = new Promise(() => {});
    const { container } = renderSection();
    expect(container).toBeEmptyDOMElement();
  });

  it('lists the files, each opening its own page, with its version', async () => {
    api.files = Promise.resolve([file]);
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
    api.files = Promise.resolve([file]);
    renderSection('guest');
    await screen.findByRole('link', { name: 'pricing.ts' });
    expect(screen.queryByRole('button', { name: /Upload/ })).toBeNull();
    expect(screen.queryByRole('button', { name: /New file/ })).toBeNull();
  });
});
