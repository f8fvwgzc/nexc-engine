import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { IssueEvent } from '@/schemas/issue';
import type { Artifact } from '@/schemas/run';

import { RunReport } from './issue-run';

const { files } = vi.hoisted(() => ({ files: { list: [] as Artifact[] } }));

vi.mock('@/features/runs/api', () => ({
  artifactsQuery: (runId: string) => ({
    queryKey: ['runs', runId, 'artifacts'],
    queryFn: () => Promise.resolve(files.list),
  }),
  downloadArtifact: vi.fn(),
  cancelRun: vi.fn(),
  graphRunsQuery: (graphId: string) => ({
    queryKey: ['graphs', 'detail', graphId, 'runs'],
    queryFn: () => Promise.resolve([]),
  }),
}));

const RUN = '0199bbbb-0000-7000-8000-00000000b001';

const report = (over: Partial<IssueEvent> = {}): IssueEvent => ({
  id: '0199bbbb-0000-7000-8000-00000000e001',
  issue_id: '0199bbbb-0000-7000-8000-00000000c001',
  kind: 'run',
  actor: null,
  body: '## Findings\n\nCAN FD fits the wrist joint.',
  from: null,
  to: 'succeeded',
  created_at: '2026-10-20T09:00:00Z',
  edited_at: null,
  run_id: RUN,
  ...over,
});

const file: Artifact = {
  id: '0199bbbb-0000-7000-8000-00000000a001',
  run_id: RUN,
  node_id: '0199bbbb-0000-7000-8000-00000000d001',
  path: 'final-deliverable.docx',
  size: 2048,
  mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  created_at: '2026-10-20T09:00:00Z',
  quarantined_at: null,
  quarantine_reason: '',
  released_at: null,
};

function renderReport(event: IssueEvent) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ol>
          <RunReport event={event} />
        </ol>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  files.list = [];
});

describe('RunReport', () => {
  it('shows the deliverable, its files and the way to the run', async () => {
    files.list = [file];
    renderReport(report());
    expect(screen.getByText('Agents')).toBeInTheDocument();
    expect(screen.getByText('finished the work')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Findings' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Open run' })).toHaveAttribute(
      'href',
      `/app/runs/${RUN}`,
    );
    expect(await screen.findByRole('button', { name: /final-deliverable\.docx/ })).toBeEnabled();
  });

  it('says a run failed, and holds back a file the classifier kept in', async () => {
    files.list = [{ ...file, quarantined_at: '2026-10-20T09:01:00Z', quarantine_reason: 'PII' }];
    renderReport(report({ to: 'failed', body: '**Fetch** failed: timed out' }));
    expect(screen.getByText('could not finish the work')).toBeInTheDocument();
    expect(screen.getByText(/timed out/)).toBeInTheDocument();
    expect(await screen.findByRole('button', { name: /final-deliverable\.docx/ })).toBeDisabled();
  });

  it('has no link once the run is gone', () => {
    renderReport(report({ run_id: null }));
    expect(screen.queryByRole('link', { name: 'Open run' })).not.toBeInTheDocument();
  });
});
