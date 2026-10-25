import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { Agent } from '@/schemas/agent';
import type { Issue, IssueEvent } from '@/schemas/issue';
import type { Team, Workspace, WorkspaceMember } from '@/schemas/workspace';
import { useAuthStore } from '@/stores/auth-store';
import * as f from '@/test/fixtures';

import { IssueTimeline } from './issue-timeline';

const { listEvents, createComment, updateComment, deleteComment, addReaction, removeReaction } =
  vi.hoisted(() => ({
    listEvents: vi.fn(),
    createComment: vi.fn(),
    updateComment: vi.fn(),
    deleteComment: vi.fn(),
    addReaction: vi.fn(),
    removeReaction: vi.fn(),
  }));

vi.mock('./api', () => ({
  issueEventsQuery: (issueId: string) => ({
    queryKey: ['issues', 'events', issueId],
    queryFn: listEvents,
  }),
  createComment,
  updateComment,
  deleteComment,
  addReaction,
  removeReaction,
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
    reactions: [],
    parent_id: null,
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

/** Opens the menu of a comment by `author` and picks Edit. */
async function edit(author = me.name) {
  const menu = await screen.findByRole('button', { name: `Actions for the comment by ${author}` });
  fireEvent.keyDown(menu, { key: 'Enter' });
  fireEvent.click(await screen.findByRole('menuitem', { name: 'Edit' }));
}

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
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    expect(await screen.findByText('Latency budget: 1 ms.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Show: All activity' })).toBeInTheDocument();
    expect(createComment).toHaveBeenCalledWith(issue.id, 'Latency budget: 1 ms.');
  });
});

describe('a comment edit in progress', () => {
  async function startEditing() {
    renderTimeline();
    await edit();
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

    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
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
    await edit();
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
    await edit();
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

describe('reactions', () => {
  const grace = { user_id: GRACE, name: 'Grace Hopper' };
  const liked = event('5', {
    actor: grace,
    body: 'Ship it.',
    reactions: [{ emoji: '👍', count: 1, reacted: false, users: [grace] }],
  });

  beforeEach(() => {
    addReaction.mockReset();
    removeReaction.mockReset();
  });

  it('show at once when clicked, then what the server answers', async () => {
    listEvents.mockResolvedValue([liked]);
    let answer: (reactions: IssueEvent['reactions']) => void = () => {};
    addReaction.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    renderTimeline();
    const chip = await screen.findByRole('button', { name: '👍 1' });
    expect(chip).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByText('Grace Hopper reacted')).toBeInTheDocument();

    fireEvent.click(chip);
    // Before the server answers.
    expect(await screen.findByRole('button', { name: '👍 2' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(screen.getByText('Grace Hopper and you reacted')).toBeInTheDocument();
    expect(addReaction).toHaveBeenCalledWith(issue.id, liked.id, '👍');

    // The server knows of a third person who reacted meanwhile.
    const alan = { user_id: '0199bbbb-0000-7000-8000-00000000a003', name: 'Alan Turing' };
    answer([{ emoji: '👍', count: 3, reacted: true, users: [me, alan, grace] }]);
    expect(await screen.findByRole('button', { name: '👍 3' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(screen.getByText('Alan Turing and 2 others reacted')).toBeInTheDocument();
  });

  it('come back as they were when the server refuses', async () => {
    const mineLiked = event('5', {
      body: 'Ship it.',
      reactions: [{ emoji: '🎉', count: 1, reacted: true, users: [me] }],
    });
    listEvents.mockResolvedValue([mineLiked]);
    let refuse: (error: Error) => void = () => {};
    removeReaction.mockReturnValue(new Promise((_, reject) => (refuse = reject)));
    renderTimeline();
    fireEvent.click(await screen.findByRole('button', { name: '🎉 1' }));
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: '🎉 1' })).not.toBeInTheDocument(),
    );
    expect(removeReaction).toHaveBeenCalledWith(issue.id, mineLiked.id, '🎉');

    refuse(new Error('Not allowed'));
    expect(await screen.findByRole('button', { name: '🎉 1' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(screen.getByText('You reacted')).toBeInTheDocument();
  });

  it('are added from the picker of allowed emoji', async () => {
    addReaction.mockResolvedValue([{ emoji: '🚀', count: 1, reacted: true, users: [me] }]);
    renderTimeline();
    fireEvent.click(await screen.findByRole('button', { name: 'Add a reaction' }));
    const picker = await screen.findByRole('group', { name: 'Pick a reaction' });
    expect(within(picker).getAllByRole('button')).toHaveLength(10);
    fireEvent.click(within(picker).getByRole('button', { name: 'rocket' }));

    expect(await screen.findByRole('button', { name: '🚀 1' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(addReaction).toHaveBeenCalledWith(issue.id, mine.id, '🚀');
  });
});

describe('replies', () => {
  const grace = { user_id: GRACE, name: 'Grace Hopper' };
  const answer = event('6', {
    actor: grace,
    body: 'Agreed, CAN FD.',
    parent_id: mine.id,
    created_at: '2026-10-20T09:30:00Z',
  });

  beforeEach(() => createComment.mockReset());

  it('show in their comment’s thread, and Enter sends one there', async () => {
    const posted = event('7', { body: 'Ordering the parts.', parent_id: mine.id });
    listEvents.mockResolvedValue([moved, mine, answer]);
    createComment.mockResolvedValue(posted);
    renderTimeline();
    const box = await screen.findByRole('textbox', { name: `Reply to ${me.name}` });
    const thread = box.closest('li');
    expect(thread).toHaveTextContent('Agreed, CAN FD.');

    listEvents.mockResolvedValue([moved, mine, answer, posted]);
    fireEvent.change(box, { target: { value: 'Ordering the parts.' } });
    fireEvent.keyDown(box, { key: 'Enter' });

    await waitFor(() =>
      expect(createComment).toHaveBeenCalledWith(issue.id, 'Ordering the parts.', mine.id),
    );
    await waitFor(() => expect(thread).toHaveTextContent('Ordering the parts.'));
    expect(screen.getByRole('textbox', { name: `Reply to ${me.name}` })).toHaveValue('');
    // Shift+Enter is a new line, not a reply.
    expect(createComment).toHaveBeenCalledTimes(1);
  });

  it('open from Reply with the focus in the box, which Escape closes while empty', async () => {
    renderTimeline();
    fireEvent.click(await screen.findByRole('button', { name: 'Reply' }));
    const box = await screen.findByRole('textbox', { name: `Reply to ${me.name}` });
    await waitFor(() => expect(box).toHaveFocus());
    fireEvent.keyDown(box, { key: 'Enter', shiftKey: true });
    expect(createComment).not.toHaveBeenCalled();

    fireEvent.keyDown(box, { key: 'Escape' });
    await waitFor(() =>
      expect(screen.queryByRole('textbox', { name: /^Reply to/ })).not.toBeInTheDocument(),
    );
  });

  it('to a reply start by mentioning its author', async () => {
    listEvents.mockResolvedValue([mine, answer]);
    createComment.mockResolvedValue(event('7', { body: 'ok', parent_id: mine.id }));
    renderTimeline();
    fireEvent.click(await screen.findByRole('button', { name: 'Reply to Grace Hopper' }));
    const box = screen.getByRole('textbox', { name: `Reply to ${me.name}` });
    await waitFor(() => expect(box).toHaveValue('@Grace Hopper '));
    // The caret waits after the mention.
    await waitFor(() => expect(box).toHaveFocus());
    expect((box as HTMLTextAreaElement).selectionStart).toBe('@Grace Hopper '.length);

    fireEvent.change(box, { target: { value: '@Grace Hopper ordered.' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send reply' }));
    await waitFor(() =>
      expect(createComment).toHaveBeenCalledWith(
        issue.id,
        `@[Grace Hopper](user:${GRACE}) ordered.`,
        mine.id,
      ),
    );
  });
});

describe('changes', () => {
  it('one person made in a row read as one entry, under its first change', async () => {
    const priority = event('8', {
      kind: 'priority',
      to: 'High',
      created_at: '2026-10-20T09:02:00Z',
    });
    listEvents.mockResolvedValue([moved, priority, mine]);
    renderTimeline();
    const first = await screen.findByText(/moved this from Backlog to Todo/);
    expect(first.closest('li')).toHaveTextContent('set priority to High');
    expect(screen.getByText(/Use CAN FD\./).closest('li')).not.toHaveTextContent('priority');
  });
});
