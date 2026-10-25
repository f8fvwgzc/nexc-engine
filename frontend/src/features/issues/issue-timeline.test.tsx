import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { Agent } from '@/schemas/agent';
import type { Issue, IssueEvent } from '@/schemas/issue';
import type { Team, Workspace, WorkspaceMember } from '@/schemas/workspace';
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
const WORKSPACE = '0199bbbb-0000-7000-8000-00000000f001';
const TEAM = '0199bbbb-0000-7000-8000-00000000f002';
const GRACE = '0199bbbb-0000-7000-8000-00000000a001';
const GUS = '0199bbbb-0000-7000-8000-00000000a002';

const workspace: Workspace = {
  id: WORKSPACE,
  name: 'Acme',
  slug: 'acme',
  role: 'member',
  member_count: 3,
  created_at: at,
};
const team: Team = {
  id: TEAM,
  workspace_id: WORKSPACE,
  name: 'Engineering',
  key: 'ENG',
  description: '',
  private: false,
  role: 'member',
  member_count: 2,
  created_at: at,
};
const member = (user_id: string, name: string, role: WorkspaceMember['role']) => ({
  user_id,
  name,
  email: `${name.split(' ')[0]?.toLowerCase()}@acme.example`,
  role,
  suspended: false,
  joined_at: at,
});
// Gus is a guest outside the team: he cannot see the issue, so he is not offered.
const members: WorkspaceMember[] = [
  member(f.user.id, f.user.name, 'owner'),
  member(GRACE, 'Grace Hopper', 'member'),
  member(GUS, 'Gus Guest', 'guest'),
];
const researcher: Agent = { ...f.agent, name: 'Researcher', role: 'researcher' };
const quarantined: Agent = {
  ...f.agent,
  id: '0199bbbb-0000-7000-8000-00000000b002',
  name: 'Rogue',
  status: 'quarantined',
};
const paused: Agent = {
  ...f.agent,
  id: '0199bbbb-0000-7000-8000-00000000b003',
  name: 'Scribe',
  role: 'writer',
  status: 'paused',
};

vi.mock('@/features/workspaces/api', () => ({
  workspacesQuery: () => ({ queryKey: ['workspaces'], queryFn: () => [workspace] }),
  teamsQuery: (wid: string) => ({ queryKey: ['workspaces', wid, 'teams'], queryFn: () => [team] }),
  membersQuery: (wid: string) => ({
    queryKey: ['workspaces', wid, 'members'],
    queryFn: () => members,
  }),
  teamMembersQuery: (wid: string, tid: string) => ({
    queryKey: ['workspaces', wid, 'teams', tid, 'members'],
    queryFn: () => [],
  }),
}));
vi.mock('@/features/agents/api', () => ({
  agentsQuery: (wid: string) => ({
    queryKey: ['agents', wid],
    queryFn: () => [researcher, quarantined, paused],
  }),
}));

const issue: Issue = {
  id: '0199bbbb-0000-7000-8000-00000000c001',
  workspace_id: WORKSPACE,
  team_id: TEAM,
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
      <MemoryRouter>
        <IssueTimeline issue={issue} canModerate={false} />
      </MemoryRouter>
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

describe('mentions', () => {
  const grace = `@[Grace Hopper](user:${GRACE})`;
  const agent = `@[Researcher](agent:${researcher.id})`;

  it('are picked from a list that opens on @ and sent as tokens', async () => {
    createComment.mockResolvedValue(event('3', { body: 'sent' }));
    renderTimeline();
    const field = screen.getByRole('textbox', { name: 'New comment' });
    fireEvent.focus(field);
    fireEvent.change(field, { target: { value: 'Ask @g' } });

    // Only people who can see the issue are offered: not Gus, a guest outside the team, nor me.
    expect(await screen.findByRole('option', { name: 'Grace Hopper' })).toBeInTheDocument();
    expect(screen.queryByRole('option', { name: /Gus Guest/ })).not.toBeInTheDocument();
    fireEvent.keyDown(field, { key: 'Enter' });
    expect(field).toHaveValue('Ask @Grace Hopper ');
    await waitFor(() => expect(screen.queryByRole('listbox')).not.toBeInTheDocument());

    fireEvent.change(field, { target: { value: 'Ask @Grace Hopper and @' } });
    expect(await screen.findByRole('option', { name: /Researcher/ })).toBeInTheDocument();
    // Quarantined agents take no work and are not offered; a paused one says it takes none.
    expect(screen.queryByRole('option', { name: /Rogue/ })).not.toBeInTheDocument();
    expect(screen.getByRole('option', { name: /Scribe/ })).toHaveTextContent(
      "writer · paused, won't take work",
    );
    fireEvent.keyDown(field, { key: 'ArrowDown' });
    expect(screen.getByRole('option', { name: /Researcher/ })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    fireEvent.keyDown(field, { key: 'Tab' });
    expect(field).toHaveValue('Ask @Grace Hopper and @Researcher ');

    fireEvent.click(screen.getByRole('button', { name: 'Comment' }));
    await waitFor(() =>
      expect(createComment).toHaveBeenCalledWith(issue.id, `Ask ${grace} and ${agent}`),
    );
  });

  it('close with Escape and leave the text as typed', async () => {
    createComment.mockResolvedValue(event('3', { body: 'sent' }));
    renderTimeline();
    const field = screen.getByRole('textbox', { name: 'New comment' });
    fireEvent.focus(field);
    fireEvent.change(field, { target: { value: '@gr' } });
    await screen.findByRole('option', { name: 'Grace Hopper' });
    fireEvent.keyDown(field, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('listbox')).not.toBeInTheDocument());
    // Cmd+Enter sends what is there; a name nobody picked stays plain text.
    fireEvent.keyDown(field, { key: 'Enter', metaKey: true });
    await waitFor(() => expect(createComment).toHaveBeenCalledWith(issue.id, '@gr'));
  });

  it('show as chips: a person by their current name, an agent as a link to it', async () => {
    const old = `@[G. Hopper](user:${GRACE})`;
    listEvents.mockResolvedValue([event('4', { body: `${old} and ${agent}: Use CAN FD.` })]);
    renderTimeline();
    expect(await screen.findByText('Grace Hopper')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Researcher' })).toHaveAttribute(
      'href',
      `/app/agents?agent=${researcher.id}`,
    );
    expect(screen.getByText(/Use CAN FD\./)).toBeInTheDocument();
    expect(screen.queryByText(/user:/)).not.toBeInTheDocument();
  });

  it('of nobody the reader knows show as plain text', async () => {
    const stranger = '0199bbbb-0000-7000-8000-00000000dead';
    const body = `@[Platform Admin](user:${stranger}) and @[Ghost](agent:${stranger}) look`;
    listEvents.mockResolvedValue([event('4', { body })]);
    renderTimeline();
    // Once the members and agents are known, neither is a chip: no card, no link.
    await waitFor(() =>
      expect(screen.getByText(/look$/)).toHaveTextContent('@Platform Admin and @Ghost look'),
    );
    expect(screen.queryByRole('link', { name: 'Ghost' })).not.toBeInTheDocument();
  });

  it('are sent once when Cmd+Enter is pressed again while an edit is saved', async () => {
    updateComment.mockReset();
    updateComment.mockReturnValue(new Promise(() => {}));
    renderTimeline();
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
    fireEvent.change(editField()!, { target: { value: 'Use EtherCAT.' } });
    fireEvent.keyDown(editField()!, { key: 'Enter', metaKey: true });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled());
    fireEvent.keyDown(editField()!, { key: 'Enter', metaKey: true });
    expect(updateComment).toHaveBeenCalledTimes(1);
  });

  it('are kept through an edit', async () => {
    updateComment.mockResolvedValue(event('2', { body: 'edited' }));
    listEvents.mockResolvedValue([event('2', { body: `${grace} check this` })]);
    renderTimeline();
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
    expect(editField()).toHaveValue('@Grace Hopper check this');
    fireEvent.change(editField()!, { target: { value: '@Grace Hopper check this again' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(updateComment).toHaveBeenCalledWith(
        issue.id,
        event('2', {}).id,
        `${grace} check this again`,
      ),
    );
  });
});
