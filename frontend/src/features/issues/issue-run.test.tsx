import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Issue, IssueEvent } from '@/schemas/issue';
import type { SseEvent } from '@/schemas/realtime';
import type { Artifact, Run } from '@/schemas/run';

import { IssueRun, RunReport } from './issue-run';

const { files, stream } = vi.hoisted(() => ({
  files: { list: [] as Artifact[] },
  /** What the run stream hands its events to; each test sends them by hand. */
  stream: { onEvent: null as ((event: SseEvent) => void) | null },
}));

vi.mock('@/lib/realtime/sse', () => ({
  GraphEventStream: class {
    start = vi.fn();
    stop = vi.fn();
    constructor(_graphId: string, opts: { onEvent: (event: SseEvent) => void }) {
      stream.onEvent = opts.onEvent;
    }
  },
}));

vi.mock('@/features/graphs/api', () => ({
  graphQuery: (graphId: string) => ({
    queryKey: ['graphs', 'detail', graphId],
    queryFn: () => Promise.resolve({ id: graphId, nodes: [] }),
  }),
}));

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
  stream.onEvent = null;
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

describe('IssueRun', () => {
  const issue = {
    id: '0199bbbb-0000-7000-8000-00000000c001',
    identifier: 'ENG-7',
    title: 'Pick the bus',
    graph_id: '0199bbbb-0000-7000-8000-00000000f001',
  } as Issue;
  const fromRuns = ['artifacts', 'runs', issue.id];

  function renderRun() {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidated = vi.spyOn(client, 'invalidateQueries');
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <IssueRun issue={issue} />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    return invalidated;
  }

  it('asks for the issue’s files from runs again when a run makes one', async () => {
    const invalidated = renderRun();
    await screen.findByRole('region', { name: 'Agent work' });

    act(() => stream.onEvent?.({ type: 'artifact.created', id: null, data: { artifact: file } }));

    expect(invalidated).toHaveBeenCalledWith({ queryKey: fromRuns });
  });

  it('asks for them again when a run finishes, so each shows the run’s end', async () => {
    const invalidated = renderRun();
    await screen.findByRole('region', { name: 'Agent work' });
    const run = { id: RUN, status: 'succeeded', node_runs: [] } as unknown as Run;

    act(() => stream.onEvent?.({ type: 'run.finished', id: null, data: { run } }));

    expect(invalidated).toHaveBeenCalledWith({ queryKey: fromRuns });
  });
});
