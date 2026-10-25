import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { qk } from '@/lib/query-keys';
import type { Issue } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import { dueLabel, localToday } from './due';
import { IssueDetail } from './issue-detail';

const { updateIssue, deleteIssue, empty, slow } = vi.hoisted(() => ({
  updateIssue: vi.fn(),
  deleteIssue: vi.fn(),
  /** A query that reads an empty list, under its own key. */
  empty: (key: string) => () => ({ queryKey: [key], queryFn: () => Promise.resolve([]) }),
  /** Whether the team's states are still on their way (they never arrive). */
  slow: { states: false },
}));

vi.mock('./api', () => ({
  updateIssue,
  deleteIssue,
  statesQuery: () => ({
    queryKey: ['states', slow.states],
    queryFn: () => (slow.states ? new Promise(() => undefined) : Promise.resolve([])),
  }),
  projectsQuery: empty('projects'),
  cyclesQuery: empty('cycles'),
}));
vi.mock('@/features/workspaces/api', () => ({
  teamsQuery: empty('teams'),
  membersQuery: empty('members'),
}));
vi.mock('@/features/ontology/api', () => ({ ontologyStatusQuery: empty('ontology') }));
// The sections that read their own data are not what these tests are about.
vi.mock('./issue-run', () => ({ IssueRun: () => null }));
vi.mock('./issue-timeline', () => ({ IssueTimeline: () => null }));
vi.mock('./sub-issues', () => ({ SubIssues: () => null }));
vi.mock('./label-picker', () => ({ LabelPicker: () => null }));

beforeAll(() => {
  // jsdom has no scrolling; the pickers' lists scroll the picked option into view.
  Element.prototype.scrollIntoView = () => undefined;
});
afterEach(() => {
  localStorage.clear();
  slow.states = false;
});

const at = '2026-10-20T09:00:00Z';

const workspace: Workspace = {
  id: '0199bbbb-0000-7000-8000-00000000f001',
  name: 'Acme',
  slug: 'acme',
  role: 'member',
  member_count: 2,
  created_at: at,
};

/** The copy of the issue a list read a while ago. */
const listed: Issue = {
  id: '0199bbbb-0000-7000-8000-00000000c001',
  workspace_id: workspace.id,
  team_id: '0199bbbb-0000-7000-8000-00000000f002',
  identifier: 'ENG-7',
  number: 7,
  title: 'Pick the wrist bus',
  description: 'Compare CAN FD and EtherCAT.',
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

/** The issue as it is now: an agent rewrote the description since the list was read. */
const fresh: Issue = {
  ...listed,
  description: 'Compare CAN FD and EtherCAT. Latency budget: 1 ms.',
  updated_at: '2026-10-20T10:00:00Z',
};

function renderDetail(first: Issue, artifacts?: ReactNode, firstPending = false) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const ui = (copy: Issue, pending: boolean) => (
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <IssueDetail
          workspace={workspace}
          issue={copy}
          descriptionPending={pending}
          artifacts={artifacts}
        />
      </MemoryRouter>
    </QueryClientProvider>
  );
  const view = render(ui(first, firstPending));
  return {
    client,
    rerender: (copy: Issue, pending = false) => view.rerender(ui(copy, pending)),
  };
}

const titleField = () => screen.getByRole('textbox', { name: 'Title' });
const descriptionField = () => screen.getByRole('textbox', { name: 'Description' });

describe('the title and description of an issue', () => {
  beforeEach(() => {
    updateIssue.mockReset();
  });

  it('send nothing when focused and left, after a newer copy replaced the listed one', () => {
    const { client, rerender } = renderDetail(listed);
    rerender(fresh);
    expect(descriptionField()).toHaveValue(fresh.description);

    for (const field of [titleField(), descriptionField()]) {
      fireEvent.focus(field);
      fireEvent.blur(field);
    }
    // A save would have started by now, even before its request goes out.
    expect(client.getMutationCache().getAll()).toHaveLength(0);
  });

  it('save only the field that was edited', async () => {
    updateIssue.mockResolvedValue({ ...fresh, title: 'Pick the wrist bus by Friday' });
    const { client, rerender } = renderDetail(listed);
    rerender(fresh);

    fireEvent.change(titleField(), { target: { value: '  Pick the wrist bus by Friday ' } });
    fireEvent.blur(titleField());
    await waitFor(() => expect(client.isMutating()).toBe(0));
    expect(updateIssue).toHaveBeenCalledTimes(1);
    expect(updateIssue).toHaveBeenCalledWith(listed.id, { title: 'Pick the wrist bus by Friday' });
  });

  it('follow newer copies again once the edit is saved', async () => {
    const saved = { ...listed, title: 'Pick the bus' };
    updateIssue.mockResolvedValue(saved);
    const { client, rerender } = renderDetail(listed);

    fireEvent.change(titleField(), { target: { value: 'Pick the bus' } });
    fireEvent.blur(titleField());
    rerender(saved);
    await waitFor(() => expect(client.isMutating()).toBe(0));
    // A teammate renames it afterwards: the field shows their title, not the one typed here.
    rerender({ ...saved, title: 'Pick the wrist bus (EtherCAT)' });
    expect(titleField()).toHaveValue('Pick the wrist bus (EtherCAT)');
    expect(updateIssue).toHaveBeenCalledTimes(1);
  });

  it('put the title back when it is cleared', () => {
    const { client } = renderDetail(listed);
    fireEvent.change(titleField(), { target: { value: '   ' } });
    fireEvent.blur(titleField());
    expect(client.getMutationCache().getAll()).toHaveLength(0);
    expect(titleField()).toHaveValue(listed.title);
  });

  it('keep what was typed when the save fails', async () => {
    updateIssue.mockRejectedValue(new Error('The server is down'));
    renderDetail(fresh);

    fireEvent.change(descriptionField(), { target: { value: 'Use EtherCAT.' } });
    fireEvent.blur(descriptionField());
    expect(await screen.findByRole('alert')).toBeInTheDocument();
    expect(updateIssue).toHaveBeenCalledWith(listed.id, { description: 'Use EtherCAT.' });
    expect(descriptionField()).toHaveValue('Use EtherCAT.');
  });
});

describe('a list’s copy without a description', () => {
  beforeEach(() => {
    updateIssue.mockReset();
  });

  it('holds the description back until the issue’s own copy lands, and saves nothing', () => {
    const { client, rerender } = renderDetail({ ...listed, description: '' }, undefined, true);
    expect(screen.queryByRole('textbox', { name: 'Description' })).not.toBeInTheDocument();
    expect(screen.getByRole('status', { name: 'Loading the description' })).toBeInTheDocument();
    // The title is there to edit at once.
    expect(titleField()).toHaveValue(listed.title);

    rerender(fresh);
    expect(descriptionField()).toHaveValue(fresh.description);
    fireEvent.focus(descriptionField());
    fireEvent.blur(descriptionField());
    expect(client.getMutationCache().getAll()).toHaveLength(0);
  });
});

describe('the properties panel', () => {
  beforeEach(() => {
    updateIssue.mockReset();
  });

  const panel = () => screen.queryByRole('complementary', { name: 'Properties' });

  it('is open by default, closes, and stays closed on the next issue', async () => {
    renderDetail(listed);
    expect(panel()).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Close properties' }));
    await waitFor(() => expect(panel()).not.toBeInTheDocument());
    expect(localStorage.getItem('nexc.issue.properties')).toBe('closed');
    // Only the one way back stays on the page.
    expect(screen.queryByRole('button', { name: `Delete ${listed.identifier}` })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Open properties' }));
    expect(panel()).toBeInTheDocument();
    expect(localStorage.getItem('nexc.issue.properties')).toBe('open');
  });

  it('hands the focus to the button that undoes the toggle', async () => {
    renderDetail(listed);
    fireEvent.click(screen.getByRole('button', { name: 'Close properties' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Open properties' })).toHaveFocus(),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Open properties' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Close properties' })).toHaveFocus(),
    );
  });

  it('does not take the focus when the page opens', () => {
    renderDetail(listed);
    expect(document.body).toHaveFocus();
  });

  it('opens closed when the member closed it before', () => {
    localStorage.setItem('nexc.issue.properties', 'closed');
    renderDetail(listed);
    expect(panel()).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open properties' })).toBeInTheDocument();
  });

  it('saves a picked priority and reads again only what shows it', async () => {
    const saved = { ...fresh, priority: 1 };
    updateIssue.mockResolvedValue(saved);
    const { client } = renderDetail(fresh);
    const invalidated = vi.spyOn(client, 'invalidateQueries');

    fireEvent.click(screen.getByRole('button', { name: 'Priority: High' }));
    fireEvent.click(await screen.findByRole('option', { name: 'Urgent' }));
    await waitFor(() => expect(client.isMutating()).toBe(0));

    expect(updateIssue).toHaveBeenCalledWith(listed.id, { priority: 1 });
    expect(client.getQueryData(qk.issues.one(listed.id))).toEqual(saved);
    const keys = invalidated.mock.calls.map(([filters]) => filters?.queryKey);
    expect(keys).toContainEqual(qk.issues.lists(workspace.id));
    expect(keys).toContainEqual(qk.issues.events(listed.id));
    // Not every issue query, not the issue just saved, not the inbox or the projects.
    expect(keys).not.toContainEqual(qk.issues.all);
    expect(keys).not.toContainEqual(qk.issues.one(listed.id));
    expect(keys).not.toContainEqual(qk.issues.inbox(workspace.id));
    expect(keys).not.toContainEqual(qk.issues.projectLists(workspace.id));
  });

  it('reads the inbox again when the assignee changes', async () => {
    const assigned = { ...fresh, assignee: { user_id: workspace.id, name: 'Ada Lovelace' } };
    updateIssue.mockResolvedValue({ ...assigned, assignee: null });
    const { client } = renderDetail(assigned);
    const invalidated = vi.spyOn(client, 'invalidateQueries');

    fireEvent.click(screen.getByRole('button', { name: 'Assignee: Ada Lovelace' }));
    fireEvent.click(await screen.findByRole('option', { name: 'Unassigned' }));
    await waitFor(() => expect(client.isMutating()).toBe(0));

    expect(updateIssue).toHaveBeenCalledWith(listed.id, { assignee_id: null });
    const keys = invalidated.mock.calls.map(([filters]) => filters?.queryKey);
    expect(keys).toContainEqual(qk.issues.inbox(workspace.id));
  });

  it('names the due date by what it shows, and keeps it when its day is clicked again', async () => {
    const due = { ...fresh, due_date: '2026-10-17' };
    const { client } = renderDetail(due);
    const shown = dueLabel('2026-10-17', localToday(), true).text;
    const button = screen.getByRole('button', { name: `Due date: ${shown}` });
    expect(button).toHaveTextContent(shown);

    fireEvent.click(button);
    fireEvent.click(await screen.findByRole('button', { name: /selected/ }));
    expect(client.getMutationCache().getAll()).toHaveLength(0);
  });

  it('names an unset due date by its prompt', () => {
    renderDetail(fresh);
    expect(screen.getByRole('button', { name: 'Set due date' })).toHaveTextContent('Set due date');
  });

  it('shows loading rows in a picker whose choices are on their way', async () => {
    slow.states = true;
    renderDetail(fresh);
    fireEvent.click(screen.getByRole('button', { name: 'Status: Todo' }));
    expect(await screen.findByRole('status', { name: 'Loading the choices' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Todo' })).toBeInTheDocument();
    expect(screen.queryByText('Nothing matches.')).not.toBeInTheDocument();
  });

  it('picking the value it already has saves nothing', async () => {
    const { client } = renderDetail(fresh);
    fireEvent.click(screen.getByRole('button', { name: 'Status: Todo' }));
    fireEvent.click(await screen.findByRole('option', { name: 'Todo' }));
    expect(client.getMutationCache().getAll()).toHaveLength(0);
  });
});

describe('deleting an issue', () => {
  const listFiles = vi.fn();
  const filesKey = qk.artifacts.list(workspace.id, { issue_id: listed.id });

  /** The issue's files, as its page shows them. */
  function Files() {
    useQuery({ queryKey: filesKey, queryFn: listFiles, staleTime: 60_000 });
    return null;
  }

  it('marks every file list stale, to be read where it is next shown, not on the way out', async () => {
    deleteIssue.mockResolvedValue(undefined);
    listFiles.mockResolvedValue([]);
    const { client } = renderDetail(listed, <Files />);
    await waitFor(() => expect(listFiles).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole('button', { name: `Delete ${listed.identifier}` }));
    fireEvent.click(await screen.findByRole('button', { name: 'Delete issue' }));
    await waitFor(() => expect(client.getQueryState(filesKey)?.isInvalidated).toBe(true));
    expect(deleteIssue).toHaveBeenCalledWith(listed.id);
    expect(listFiles).toHaveBeenCalledTimes(1);
  });
});
