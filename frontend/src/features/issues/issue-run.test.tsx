import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import type { RunArtifact } from '@/schemas/artifact';
import type { Issue, IssueEvent } from '@/schemas/issue';
import type { SseEvent } from '@/schemas/realtime';
import type { Artifact, Run } from '@/schemas/run';

import { IssueRun, RunReport, SubIssueRun } from './issue-run';

const { files, reads, stream } = vi.hoisted(() => ({
  /** The files the issue's runs made, as `/issues/{id}/run-artifacts` answers. */
  files: { list: [] as RunArtifact[], ofRun: [] as Artifact[] },
  /** Requests made, by path. */
  reads: { issue: 0, run: 0 },
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

vi.mock('@/features/artifacts/api', () => ({
  issueRunArtifactsQuery: (issueId: string) => ({
    queryKey: ['artifacts', 'runs', issueId],
    queryFn: () => {
      reads.issue += 1;
      return Promise.resolve(files.list);
    },
  }),
}));

vi.mock('@/features/runs/api', () => ({
  artifactsQuery: (runId: string) => ({
    queryKey: ['runs', runId, 'artifacts'],
    queryFn: () => {
      reads.run += 1;
      return Promise.resolve(files.ofRun);
    },
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
  reactions: [],
  parent_id: null,
  ...over,
});

const file: RunArtifact = {
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
  run_status: 'succeeded',
  run_created_at: '2026-10-20T08:59:00Z',
};

function renderReport(...events: IssueEvent[]) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ol>
          {events.map((event) => (
            <RunReport key={event.id} event={event} />
          ))}
        </ol>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeAll(async () => {
  // A report's Markdown is loaded with the first report; a cold import can outlast `findBy`.
  await import('@/components/custom-ui/markdown');
});

beforeEach(() => {
  files.list = [];
  files.ofRun = [];
  reads.issue = 0;
  reads.run = 0;
  stream.onEvent = null;
});

describe('RunReport', () => {
  it('shows the deliverable, its files and the way to the run', async () => {
    files.list = [file];
    renderReport(report());
    expect(screen.getByText('Agents')).toBeInTheDocument();
    expect(screen.getByText('finished the work')).toBeInTheDocument();
    expect(await screen.findByRole('heading', { name: 'Findings' })).toBeInTheDocument();
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

  it('reads the files of every run on the timeline once, and shows each report its own', async () => {
    const other = '0199bbbb-0000-7000-8000-00000000b002';
    files.list = [file, { ...file, id: `${file.id.slice(0, -1)}2`, run_id: other, path: 'b.md' }];
    renderReport(
      report(),
      report({ id: '0199bbbb-0000-7000-8000-00000000e002', run_id: other }),
      report({ id: '0199bbbb-0000-7000-8000-00000000e003', run_id: other }),
    );
    expect(await screen.findByRole('button', { name: /final-deliverable\.docx/ })).toBeEnabled();
    expect(await screen.findAllByRole('button', { name: /b\.md/ })).toHaveLength(2);
    expect(reads).toEqual({ issue: 1, run: 0 });
  });

  it('reads a run’s own files only when asked, once the shared list is full', async () => {
    const someoneElse = { ...file, run_id: '0199bbbb-0000-7000-8000-00000000b009' };
    files.list = Array.from({ length: 500 }, () => someoneElse);
    files.ofRun = [file];
    renderReport(report());
    const ask = await screen.findByRole('button', { name: /Show the run’s files/ });
    expect(reads.run).toBe(0);

    fireEvent.click(ask);
    expect(await screen.findByRole('button', { name: /final-deliverable\.docx/ })).toBeEnabled();
    expect(reads.run).toBe(1);
  });

  it('offers all the files of the oldest run a full list holds, which it may have cut short', async () => {
    const newer = { ...file, run_id: '0199bbbb-0000-7000-8000-00000000b009' };
    files.list = [...Array.from({ length: 499 }, () => newer), file];
    files.ofRun = [file, { ...file, id: `${file.id.slice(0, -1)}2`, path: 'appendix.md' }];
    renderReport(report());
    expect(await screen.findByRole('button', { name: /final-deliverable\.docx/ })).toBeEnabled();

    fireEvent.click(screen.getByRole('button', { name: /Show all the run’s files/ }));
    expect(await screen.findByRole('button', { name: /appendix\.md/ })).toBeEnabled();
    expect(reads.run).toBe(1);
  });

  it('says so when the run, read on its own, made no files', async () => {
    const someoneElse = { ...file, run_id: '0199bbbb-0000-7000-8000-00000000b009' };
    files.list = Array.from({ length: 500 }, () => someoneElse);
    renderReport(report());
    fireEvent.click(await screen.findByRole('button', { name: /Show the run’s files/ }));
    expect(await screen.findByText('The run made no files.')).toBeInTheDocument();
  });

  it('offers nothing more when the list is short enough to hold every file', async () => {
    files.list = [file];
    renderReport(report());
    await screen.findByRole('button', { name: /final-deliverable\.docx/ });
    expect(screen.queryByRole('button', { name: /run’s files/ })).not.toBeInTheDocument();
  });
});

describe('SubIssueRun', () => {
  it('shows where the latest run stands, moving while it runs', () => {
    const { rerender } = render(<SubIssueRun status="running" />);
    expect(screen.getByText('running')).toBeInTheDocument();
    rerender(<SubIssueRun status="failed" />);
    expect(screen.getByText('failed')).toBeInTheDocument();
  });

  it('shows nothing without a run', () => {
    const { container } = render(<SubIssueRun status={null} />);
    expect(container).toBeEmptyDOMElement();
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
