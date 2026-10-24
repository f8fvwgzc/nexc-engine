import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { Issue, IssueEvent } from '@/schemas/issue';
import { useAuthStore } from '@/stores/auth-store';
import * as f from '@/test/fixtures';

import { IssueTimeline } from './issue-timeline';

const { listEvents, createComment, updateComment, deleteComment } = vi.hoisted(() => ({
  listEvents: vi.fn(),
  createComment: vi.fn(),
  updateComment: vi.fn(),
  deleteComment: vi.fn(),
}));

vi.mock('./api', () => ({
  issueEventsQuery: (issueId: string) => ({
    queryKey: ['issues', 'events', issueId],
    queryFn: listEvents,
  }),
  createComment,
  updateComment,
  deleteComment,
}));
vi.mock('./issue-run', () => ({ RunReport: () => null }));

const at = '2026-10-20T09:00:00Z';

const issue: Issue = {
  id: '0199bbbb-0000-7000-8000-00000000c001',
  workspace_id: '0199bbbb-0000-7000-8000-00000000f001',
  team_id: '0199bbbb-0000-7000-8000-00000000f002',
  identifier: 'ENG-7',
  number: 7,
  title: 'Pick the wrist bus',
  description: '',
  state: {
    id: '0199bbbb-0000-7000-8000-00000000f003',
    team_id: '0199bbbb-0000-7000-8000-00000000f002',
    name: 'Todo',
    category: 'unstarted',
    color: '#94a3b8',
    position: 1,
  },
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

const me = { user_id: f.user.id, name: f.user.name };

function event(id: string, change: Partial<IssueEvent>): IssueEvent {
  return {
    id: `0199bbbb-0000-7000-8000-0000000e000${id}`,
    issue_id: issue.id,
    kind: 'comment',
    actor: me,
    body: '',
    from: null,
    to: null,
    created_at: at,
    edited_at: null,
    run_id: null,
    ...change,
  };
}

const moved = event('1', { kind: 'state', from: 'Backlog', to: 'Todo' });
const mine = event('2', { body: 'Use CAN FD.' });

function renderTimeline() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <IssueTimeline issue={issue} canModerate={false} />
    </QueryClientProvider>,
  );
}

/** Picks what the activity shows from its menu. */
async function show(label: string) {
  fireEvent.keyDown(screen.getByRole('button', { name: /^Show: / }), { key: 'Enter' });
  fireEvent.click(await screen.findByRole('menuitemradio', { name: label }));
}

const foldToggle = () => screen.getByRole('button', { name: /^Activity/ });
const editField = () => screen.queryByRole('textbox', { name: 'Edit comment' });

beforeEach(() => {
  localStorage.clear();
  useAuthStore.setState({ user: f.user });
  listEvents.mockReset();
  listEvents.mockResolvedValue([moved, mine]);
});

afterEach(() => {
  useAuthStore.setState({ user: null });
});

describe('a comment posted while the activity hides comments', () => {
  it('switches the activity to everything, so the comment shows', async () => {
    localStorage.setItem(
      'nexc.issue.activity',
      JSON.stringify({ collapsed: false, show: 'changes', order: 'asc' }),
    );
    const posted = event('3', { body: 'Latency budget: 1 ms.' });
    createComment.mockResolvedValue(posted);
    renderTimeline();
    expect(await screen.findByText(/moved this from Backlog to Todo/)).toBeInTheDocument();

    listEvents.mockResolvedValue([moved, mine, posted]);
    fireEvent.change(screen.getByRole('textbox', { name: 'New comment' }), {
      target: { value: 'Latency budget: 1 ms.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Comment' }));

    expect(await screen.findByText('Latency budget: 1 ms.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Show: All activity' })).toBeInTheDocument();
    expect(createComment).toHaveBeenCalledWith(issue.id, 'Latency budget: 1 ms.');
  });
});

describe('a comment edit in progress', () => {
  async function startEditing() {
    renderTimeline();
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
    fireEvent.change(editField()!, { target: { value: 'Use EtherCAT.' } });
  }

  it('is still there after the activity is folded and opened again', async () => {
    await startEditing();
    fireEvent.click(foldToggle());
    expect(editField()).not.toBeInTheDocument();
    fireEvent.click(foldToggle());

    expect(editField()).toHaveValue('Use EtherCAT.');
    // Coming back into view does not pull the focus into it.
    expect(editField()).not.toHaveFocus();
  });

  it('is still there after the activity shows something else for a while', async () => {
    await startEditing();
    await show('Runs');
    await waitFor(() => expect(editField()).not.toBeInTheDocument());
    await show('All activity');

    await waitFor(() => expect(editField()).toHaveValue('Use EtherCAT.'));
    expect(updateComment).not.toHaveBeenCalled();
  });
});
