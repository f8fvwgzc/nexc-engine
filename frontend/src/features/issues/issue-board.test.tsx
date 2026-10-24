import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { qk } from '@/lib/query-keys';
import type { Issue } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import { IssueBoard } from './issue-board';

const { updateIssue, listIssues, todo, doing } = vi.hoisted(() => {
  const team_id = '0199bbbb-0000-7000-8000-00000000f002';
  return {
    updateIssue: vi.fn(),
    listIssues: vi.fn(),
    todo: {
      id: '0199bbbb-0000-7000-8000-00000000f003',
      team_id,
      name: 'Todo',
      category: 'unstarted' as const,
      color: '#94a3b8',
      position: 1,
    },
    doing: {
      id: '0199bbbb-0000-7000-8000-00000000f004',
      team_id,
      name: 'In Progress',
      category: 'started' as const,
      color: '#f59e0b',
      position: 2,
    },
  };
});

vi.mock('./api', () => ({
  updateIssue,
  statesQuery: (workspaceId: string, teamId: string) => ({
    queryKey: ['issues', workspaceId, 'states', teamId],
    queryFn: () => Promise.resolve([todo, doing]),
  }),
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
  sub_issues: { total: 0, closed: 0 },
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

/** The board as the issues page shows it: fed from an issue list in the cache. */
function Board() {
  const { data = [] } = useQuery({
    queryKey: qk.issues.list(workspace.id, { team_id: todo.team_id }),
    queryFn: listIssues,
  });
  return (
    <IssueBoard
      workspace={workspace}
      teamId={todo.team_id}
      issues={data}
      hideClosed={false}
      onCreate={() => undefined}
    />
  );
}

function renderBoard() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <Board />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return client;
}

const column = (name: string) => screen.getByRole('region', { name });
const card = (name: string) =>
  within(column(name)).queryByRole('link', { name: /Pick the wrist bus/ });

/** Drops the issue's card on a column, as a drag from another column ends. */
function dropOn(name: string) {
  fireEvent.drop(column(name), { dataTransfer: { getData: () => issue.id } });
}

beforeEach(() => {
  updateIssue.mockReset();
  listIssues.mockReset();
  localStorage.clear();
});

describe('a card dropped on another column', () => {
  it('moves there at once, before the server answers', async () => {
    listIssues.mockResolvedValue([issue]);
    let answer: (moved: Issue) => void = () => undefined;
    updateIssue.mockReturnValue(new Promise<Issue>((resolve) => (answer = resolve)));
    const client = renderBoard();
    await waitFor(() => expect(card('Todo')).toBeInTheDocument());

    dropOn('In Progress');
    await waitFor(() => expect(card('In Progress')).toBeInTheDocument());
    expect(card('Todo')).not.toBeInTheDocument();
    expect(updateIssue).toHaveBeenCalledWith(issue.id, { state_id: doing.id });

    // The server's copy wins once it answers: the list is read again.
    listIssues.mockResolvedValue([{ ...issue, state: doing }]);
    answer({ ...issue, state: doing });
    await waitFor(() => expect(client.isMutating()).toBe(0));
    await waitFor(() => expect(listIssues).toHaveBeenCalledTimes(2));
    expect(card('In Progress')).toBeInTheDocument();
  });

  it('goes back to its column when the move fails', async () => {
    // The read that follows the failure never lands, so what shows is the list put back.
    listIssues.mockResolvedValueOnce([issue]).mockReturnValue(new Promise(() => undefined));
    let fail: (error: Error) => void = () => undefined;
    updateIssue.mockReturnValue(new Promise<Issue>((_, reject) => (fail = reject)));
    renderBoard();
    await waitFor(() => expect(card('Todo')).toBeInTheDocument());

    dropOn('In Progress');
    await waitFor(() => expect(card('In Progress')).toBeInTheDocument());
    fail(new Error('The server is down'));
    await waitFor(() => expect(card('Todo')).toBeInTheDocument());
    expect(card('In Progress')).not.toBeInTheDocument();
  });
});

describe('folding a column', () => {
  it('from the keyboard keeps the focus on that column’s new button', async () => {
    listIssues.mockResolvedValue([issue]);
    renderBoard();

    // A button clicked from the keyboard (Enter or Space) has `detail` 0.
    fireEvent.click(await screen.findByRole('button', { name: 'Fold Todo' }), { detail: 0 });
    expect(screen.getByRole('button', { name: /^Unfold Todo/ })).toHaveFocus();

    fireEvent.click(screen.getByRole('button', { name: /^Unfold Todo/ }), { detail: 0 });
    expect(screen.getByRole('button', { name: 'Fold Todo' })).toHaveFocus();
  });

  it('with a pointer leaves the focus alone', async () => {
    listIssues.mockResolvedValue([issue]);
    renderBoard();

    fireEvent.click(await screen.findByRole('button', { name: 'Fold Todo' }), { detail: 1 });
    expect(screen.getByRole('button', { name: /^Unfold Todo/ })).not.toHaveFocus();
  });
});
