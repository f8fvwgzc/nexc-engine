import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createMemoryRouter, RouterProvider, useLocation, useParams } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { issuePath, legacyIssuePath } from '@/features/issues/links';
import type { Issue, IssueState, Project } from '@/schemas/issue';
import type { Team, Workspace } from '@/schemas/workspace';

import IssuesPage, { IssueExplorer } from './issues-page';

const id = '6c08cc63-f95f-77d4-8576-23a9c41035ee';

/** What the mocked server answers: the teams, the issues, and each team's states. */
const { server } = vi.hoisted(() => {
  const server: { teams: Team[]; issues: Issue[]; states: Record<string, IssueState[]> } = {
    teams: [],
    issues: [],
    states: {},
  };
  return { server };
});

vi.mock('@/lib/api/client', () => ({
  apiRequest: (path: string, _schema: unknown, opts: { query?: { team_id?: string } } = {}) => {
    if (path.endsWith('/teams')) return Promise.resolve(server.teams);
    if (path.endsWith('/issues')) {
      const team = opts.query?.team_id;
      return Promise.resolve(server.issues.filter((i) => !team || i.team_id === team));
    }
    const states = /\/teams\/([^/]+)\/states$/.exec(path);
    if (states) return Promise.resolve(server.states[states[1] ?? ''] ?? []);
    return Promise.resolve([]);
  },
  apiSend: vi.fn(),
  apiDownload: vi.fn(),
}));
// The dialog is not what these tests are about.
vi.mock('@/features/issues/new-issue-dialog', () => ({ NewIssueDialog: () => null }));
// A native select, so the options offered can be read without opening a popup.
vi.mock('@/components/custom-ui/option-select', () => ({
  OptionSelect: ({
    value,
    onValueChange,
    options,
    'aria-label': label,
  }: {
    value: string;
    onValueChange: (value: string) => void;
    options: { value: string; label: ReactNode }[];
    'aria-label'?: string;
  }) => (
    <select aria-label={label} value={value} onChange={(e) => onValueChange(e.target.value)}>
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {typeof o.label === 'string' ? o.label : o.value}
        </option>
      ))}
    </select>
  ),
}));

function IssueProbe() {
  const { issueId } = useParams();
  const location = useLocation();
  return (
    <p data-testid="issue">
      {issueId} {location.search}
    </p>
  );
}

describe('old issue links', () => {
  it('name the issue page they stand for', () => {
    expect(issuePath(id)).toBe(`/app/issues/${id}`);
    expect(legacyIssuePath(new URLSearchParams(`issue=${id}`))).toBe(`/app/issues/${id}`);
    expect(legacyIssuePath(new URLSearchParams('team=eng'))).toBeNull();
    expect(legacyIssuePath(new URLSearchParams('issue='))).toBeNull();
  });

  it('open the issue on its own page, in place of the old address', () => {
    const router = createMemoryRouter(
      [
        { path: '/app/inbox', element: <p>inbox</p> },
        { path: '/app/issues', element: <IssuesPage /> },
        { path: '/app/issues/:issueId', element: <IssueProbe /> },
      ],
      { initialEntries: ['/app/inbox', `/app/issues?issue=${id}`], initialIndex: 1 },
    );
    render(<RouterProvider router={router} />);
    expect(screen.getByTestId('issue')).toHaveTextContent(id);
    expect(router.state.location.pathname).toBe(`/app/issues/${id}`);
    // Replaced, not pushed: going back skips the old address.
    expect(router.state.historyAction).toBe('REPLACE');
  });
});

describe('a project’s issues on the board', () => {
  const at = '2026-10-20T09:00:00Z';
  const workspace: Workspace = {
    id: '0199aaaa-0000-7000-8000-000000000001',
    name: 'Acme',
    slug: 'acme',
    role: 'member',
    member_count: 3,
    created_at: at,
  };
  const team = (key: string, name: string, n: number): Team => ({
    id: `0199aaaa-0000-7000-8000-00000000010${n}`,
    workspace_id: workspace.id,
    name,
    key,
    description: '',
    private: false,
    role: 'member',
    member_count: 2,
    created_at: at,
  });
  const eng = team('ENG', 'Engineering', 1);
  const des = team('DES', 'Design', 2);
  const todoOf = (t: Team, n: number): IssueState => ({
    id: `0199aaaa-0000-7000-8000-00000000020${n}`,
    team_id: t.id,
    name: 'Todo',
    category: 'unstarted',
    color: '#94a3b8',
    position: 1,
  });
  const states = { [eng.id]: [todoOf(eng, 1)], [des.id]: [todoOf(des, 2)] };
  const project: Project = {
    id: '0199aaaa-0000-7000-8000-000000000301',
    workspace_id: workspace.id,
    name: 'Wrist',
    description: '',
    status: 'started',
    team_id: eng.id,
    team: { id: eng.id, key: eng.key, name: eng.name },
    lead_id: null,
    target_date: null,
    issue_count: 3,
    closed_count: 0,
    created_at: at,
  };
  const issueOf = (t: Team, n: number, title: string): Issue => ({
    id: `0199aaaa-0000-7000-8000-00000000040${n}`,
    workspace_id: workspace.id,
    team_id: t.id,
    identifier: `${t.key}-${n}`,
    number: n,
    title,
    description: '',
    state: states[t.id]![0]!,
    priority: 0,
    labels: [],
    parent: null,
    cycle: null,
    sub_issues: { total: 0, closed: 0 },
    assignee: null,
    agent_id: null,
    project_id: project.id,
    graph_id: null,
    creator_id: null,
    due_date: null,
    github_url: null,
    created_at: at,
    updated_at: at,
    completed_at: null,
  });

  beforeEach(() => {
    server.teams = [eng, des];
    server.states = states;
    server.issues = [
      issueOf(eng, 1, 'Pick the wrist bus'),
      issueOf(eng, 2, 'Size the motors'),
      issueOf(des, 3, 'Draw the wrist cover'),
    ];
    localStorage.clear();
    localStorage.setItem('nexc.issues.view', 'board');
  });

  function renderProject() {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const router = createMemoryRouter(
      [
        {
          path: '/app/projects/:projectId',
          element: <IssueExplorer workspace={workspace} project={project} defaultTeamId={eng.id} />,
        },
      ],
      { initialEntries: [`/app/projects/${project.id}`] },
    );
    render(
      <QueryClientProvider client={client}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );
    return router;
  }

  const teamSelect = () => screen.getByRole('combobox', { name: 'Team' });

  it('names the team it shows, and offers only real teams', async () => {
    renderProject();
    await screen.findByRole('region', { name: 'Todo' });
    expect(teamSelect()).toHaveValue(eng.id);
    expect(within(teamSelect()).queryByRole('option', { name: 'All teams' })).toBeNull();
    expect(within(teamSelect()).getAllByRole('option')).toHaveLength(2);

    // The list still offers every team, and shows them all.
    fireEvent.click(screen.getByRole('button', { name: 'List view' }));
    expect(teamSelect()).toHaveValue('__all__');
    expect(within(teamSelect()).getByRole('option', { name: 'All teams' })).toBeInTheDocument();
    expect(screen.getByText('Draw the wrist cover')).toBeInTheDocument();
  });

  it('says how many issues are in other teams, and switches to one of them', async () => {
    const router = renderProject();
    const board = await screen.findByRole('region', { name: 'Todo' });
    expect(within(board).queryByText('Draw the wrist cover')).toBeNull();
    expect(screen.getByText(/1 issue here is in another team/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Show DES (1)' }));
    expect(router.state.location.search).toBe(`?team=${des.id}`);
    await waitFor(() =>
      expect(
        within(screen.getByRole('region', { name: 'Todo' })).getByText('Draw the wrist cover'),
      ).toBeInTheDocument(),
    );
    expect(teamSelect()).toHaveValue(des.id);
    expect(screen.queryByText(/in another team/)).toBeNull();
  });

  it('says nothing when every issue is on the board', async () => {
    server.issues = server.issues.filter((i) => i.team_id === eng.id);
    renderProject();
    await screen.findByText('Size the motors');
    expect(screen.queryByText(/other team/)).toBeNull();
  });
});
