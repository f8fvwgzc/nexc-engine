import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { qk } from '@/lib/query-keys';
import type { Issue, IssueState } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import { SubIssues } from './sub-issues';

const { api } = vi.hoisted(() => ({
  api: {
    list: vi.fn(),
    createIssue: vi.fn(),
    updateIssue: vi.fn(),
    states: [] as unknown[],
  },
}));

vi.mock('./api', () => ({
  issueQuery: (issueId: string) => ({
    queryKey: ['issues', 'detail', issueId],
    queryFn: () => new Promise(() => undefined),
  }),
  issuesQuery: (workspaceId: string, filter: object) => ({
    queryKey: ['issues', workspaceId, 'list', filter],
    queryFn: api.list,
  }),
  statesQuery: (workspaceId: string, teamId: string) => ({
    queryKey: ['issues', workspaceId, 'states', teamId],
    queryFn: () => Promise.resolve(api.states),
  }),
  createIssue: api.createIssue,
  updateIssue: api.updateIssue,
}));

const at = '2026-10-20T09:00:00Z';

const workspace: Workspace = {
  id: '0199bbbb-0000-7000-8000-00000000f001',
  name: 'Acme',
  slug: 'acme',
  role: 'member',
  member_count: 2,
  created_at: at,
};

const todo: IssueState = {
  id: '0199bbbb-0000-7000-8000-00000000f003',
  team_id: '0199bbbb-0000-7000-8000-00000000f002',
  name: 'Todo',
  category: 'unstarted',
  color: '#94a3b8',
  position: 1,
};
const done: IssueState = {
  ...todo,
  id: '0199bbbb-0000-7000-8000-00000000f004',
  name: 'Done',
  category: 'completed',
  position: 3,
};

const issue: Issue = {
  id: '0199bbbb-0000-7000-8000-00000000c001',
  workspace_id: workspace.id,
  team_id: todo.team_id,
  identifier: 'ENG-7',
  number: 7,
  title: 'Pick the wrist bus',
  description: '',
  state: todo,
  priority: 2,
  labels: [],
  parent: null,
  cycle: null,
  sub_issues: { total: 2, closed: 0 },
  assignee: null,
  agent_id: null,
  project_id: null,
  graph_id: null,
  creator_id: null,
  due_date: null,
  github_url: null,
  created_at: at,
  updated_at: at,
  completed_at: null,
};

/** A sub-issue as the slim issue list carries it: no description, its latest run's status. */
const part = (n: number, over: Partial<Issue> & { run_status?: string | null } = {}) => {
  const { description: _description, ...rest } = issue;
  return {
    ...rest,
    id: `0199bbbb-0000-7000-8000-00000000d00${n}`,
    identifier: `ENG-${7 + n}`,
    number: 7 + n,
    title: `Part ${n}`,
    parent: { id: issue.id, identifier: issue.identifier, title: issue.title },
    sub_issues: { total: 0, closed: 0 },
    created_at: `2026-10-20T09:0${n}:00Z`,
    run_status: null,
    ...over,
  };
};

function renderParts(of: Issue = issue) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(qk.issues.one(of.id), of);
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <SubIssues workspace={workspace} issue={of} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return client;
}

beforeEach(() => {
  api.list.mockReset();
  api.createIssue.mockReset();
  api.updateIssue.mockReset();
  api.states = [todo, done];
});

describe('SubIssues', () => {
  it('lists the parts in the order they were added, each with its latest run from the list', async () => {
    api.list.mockResolvedValue([part(2, { graph_id: issue.id, run_status: 'running' }), part(1)]);
    renderParts();
    const links = await screen.findAllByRole('link');
    expect(links.map((l) => l.textContent)).toEqual(['ENG-8Part 1', 'ENG-9Part 2']);
    expect(screen.getByText('running')).toBeInTheDocument();
    expect(
      screen.getByRole('progressbar', { name: '0 of 2 sub-issues closed' }),
    ).toBeInTheDocument();
    // One read for the whole list: no request per row.
    expect(api.list).toHaveBeenCalledTimes(1);
  });

  it('shows a run status only when the list carries one', async () => {
    const { run_status: _gone, ...older } = part(1, { graph_id: issue.id });
    api.list.mockResolvedValue([older]);
    renderParts();
    await screen.findByRole('link', { name: /Part 1/ });
    expect(screen.queryByText('running')).not.toBeInTheDocument();
  });

  it('files a part on Enter, shows it at once, and keeps the field for the next', async () => {
    api.list.mockResolvedValue([]);
    let answer: (created: Issue) => void = () => undefined;
    api.createIssue.mockReturnValue(new Promise<Issue>((resolve) => (answer = resolve)));
    const client = renderParts({ ...issue, sub_issues: { total: 0, closed: 0 } });

    fireEvent.click(screen.getByRole('button', { name: 'Add sub-issue' }));
    const field = screen.getByRole('textbox', { name: 'Sub-issue title' });
    fireEvent.change(field, { target: { value: 'Measure latency' } });
    fireEvent.keyDown(field, { key: 'Enter' });

    await waitFor(() =>
      expect(api.createIssue).toHaveBeenCalledWith(workspace.id, issue.team_id, {
        title: 'Measure latency',
        parent_id: issue.id,
      }),
    );
    expect(await screen.findByText('Adding…')).toBeInTheDocument();
    expect(screen.getByText('Measure latency')).toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'Sub-issue title' })).toHaveValue('');

    answer({
      ...issue,
      id: '0199bbbb-0000-7000-8000-00000000d009',
      identifier: 'ENG-16',
      title: 'Measure latency',
      parent: { id: issue.id, identifier: 'ENG-7', title: issue.title },
      sub_issues: { total: 0, closed: 0 },
    });
    expect(await screen.findByRole('link', { name: /ENG-16/ })).toBeInTheDocument();
    expect(screen.queryByText('Adding…')).not.toBeInTheDocument();
    // The issue counts its new part without being read again.
    expect(client.getQueryData<Issue>(qk.issues.one(issue.id))?.sub_issues).toEqual({
      total: 1,
      closed: 0,
    });

    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Sub-issue title' }), {
      key: 'Escape',
    });
    expect(screen.queryByRole('textbox', { name: 'Sub-issue title' })).not.toBeInTheDocument();
  });

  it('moves a part to another state from its glyph, and counts it closed at once', async () => {
    api.list.mockResolvedValue([part(1)]);
    api.updateIssue.mockImplementation((id: string) =>
      Promise.resolve({ ...part(1), id, state: done, description: '' }),
    );
    const client = renderParts();

    const glyph = await screen.findByRole('button', { name: 'Status of ENG-8: Todo' });
    fireEvent.keyDown(glyph, { key: 'Enter' });
    fireEvent.click(await screen.findByRole('menuitemradio', { name: 'Done' }));

    await waitFor(() =>
      expect(api.updateIssue).toHaveBeenCalledWith(part(1).id, { state_id: done.id }),
    );
    expect(client.getQueryData<Issue>(qk.issues.one(issue.id))?.sub_issues.closed).toBe(1);
    await waitFor(() => expect(client.isMutating()).toBe(0));
  });
});
