import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ChevronRightIcon,
  CircleDotIcon,
  KanbanIcon,
  ListIcon,
  NetworkIcon,
  PlusIcon,
  UsersIcon,
} from 'lucide-react';
import { useEffect, useState, type ReactNode } from 'react';
import { Link, Navigate, useSearchParams } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { GithubChip } from '@/components/custom-ui/github-chip';
import { IconButton } from '@/components/custom-ui/icon-button';
import {
  LabelChip,
  PersonGlyph,
  PriorityGlyph,
  StateGlyph,
} from '@/components/custom-ui/issue-glyphs';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { Switch } from '@/components/ui/switch';
import { cyclesQuery, issuesQuery, labelsQuery } from '@/features/issues/api';
import { DueChip } from '@/features/issues/due-chip';
import { IssueBoard } from '@/features/issues/issue-board';
import { issueLinkTitle, issuePath, legacyIssuePath } from '@/features/issues/links';
import { NewIssueDialog } from '@/features/issues/new-issue-dialog';
import { teamsQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { formatRelative } from '@/lib/format';
import { onIntent, prefetchIssue } from '@/lib/prefetch';
import { cn } from '@/lib/utils';
import { cycleLabel } from '@/schemas/cycle';
import type { Issue, Project, StateCategory } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

const VIEW_KEY = 'nexc.issues.view';
type View = 'list' | 'board';

function rememberedView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === 'board' ? 'board' : 'list';
  } catch {
    return 'list';
  }
}

const ALL = '__all__';

/** Order in which groups of states are listed: what is being worked on first. */
const CATEGORY_ORDER: StateCategory[] = [
  'started',
  'unstarted',
  'backlog',
  'completed',
  'canceled',
];

/**
 * One issue in the list. The whole row opens the issue's page (its title is the link, stretched
 * over the row, and its tooltip names what the glyphs under it show); the GitHub chip opens
 * GitHub instead.
 */
function IssueRow({ issue }: { issue: Issue }) {
  const queryClient = useQueryClient();
  return (
    <li
      {...onIntent(() => prefetchIssue(queryClient, issue))}
      className="relative flex h-9 items-center gap-2.5 px-3 text-[13px] transition-colors hover:bg-muted/60 has-[a[data-row-link]:focus-visible]:bg-muted/60"
    >
      <PriorityGlyph priority={issue.priority} />
      <span className="hidden w-16 shrink-0 font-mono text-xs text-muted-foreground sm:block">
        {issue.identifier}
      </span>
      <StateGlyph category={issue.state.category} color={issue.state.color} />
      <Link
        to={issuePath(issue.id)}
        data-row-link
        title={issueLinkTitle(issue)}
        className="min-w-0 flex-1 truncate outline-none after:absolute after:inset-0 focus-visible:after:ring-2 focus-visible:after:ring-ring/50 focus-visible:after:ring-inset"
      >
        <span className="sr-only">{issue.identifier} </span>
        {issue.title}
        {issue.parent && (
          <span className="ml-2 text-xs text-muted-foreground">
            <ChevronRightIcon className="mr-0.5 inline size-3" aria-hidden />
            {issue.parent.title}
          </span>
        )}
      </Link>
      {issue.github_url && (
        <GithubChip url={issue.github_url} className="hidden max-w-48 sm:inline-flex" />
      )}
      {issue.sub_issues.total > 0 && (
        <span
          className="shrink-0 rounded-full border px-1.5 text-xs text-muted-foreground tabular-nums"
          aria-label={`${issue.sub_issues.closed} of ${issue.sub_issues.total} sub-issues closed`}
        >
          {issue.sub_issues.closed}/{issue.sub_issues.total}
        </span>
      )}
      <span className="hidden max-w-[40%] shrink-0 items-center gap-1 overflow-hidden md:flex">
        {issue.labels.map((label) => (
          <LabelChip key={label.id} name={label.name} color={label.color} />
        ))}
      </span>
      {issue.graph_id && (
        <NetworkIcon className="size-3.5 shrink-0 text-muted-foreground" aria-label="Has a graph" />
      )}
      {issue.due_date && <DueChip issue={issue} />}
      <span className="hidden shrink-0 text-xs text-muted-foreground tabular-nums sm:block">
        {formatRelative(issue.updated_at)}
      </span>
      <PersonGlyph name={issue.assignee?.name} />
    </li>
  );
}

/** Rows shaped like the list, shown while it loads. */
function IssueListSkeleton() {
  return (
    <div
      className="overflow-hidden rounded-lg border"
      role="status"
      aria-busy
      aria-label="Loading issues"
    >
      <Skeleton className="h-8 w-full rounded-none" />
      <ul className="divide-y">
        {Array.from({ length: 8 }, (_, i) => (
          <li key={i} className="flex h-9 items-center gap-2.5 px-3">
            <Skeleton className="size-3.5 rounded" />
            <Skeleton className="h-3 w-12" />
            <Skeleton className="size-3.5 rounded-full" />
            <Skeleton className="h-3 flex-1" style={{ maxWidth: `${40 + ((i * 13) % 45)}%` }} />
            <Skeleton className="ml-auto size-5 rounded-full" />
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * The issues of a workspace as a list grouped by state or as a team's board, with filters. Given
 * a `project` it shows only that project's issues and files new ones in it; given a `person` it
 * shows only what is assigned to, or was filed by, that member, under the caller's own heading.
 * An issue opens on its own page. On the board the Team select names the board's team, and a note
 * above it counts the issues read that belong to other teams, with a way to show each of them.
 *
 * The board fills the height under the header and scrolls inside itself. Embedded in another
 * page, the explorer then needs a parent that is a flex column as tall as the space left: a
 * `PageContainer` turns into one by itself while the explorer shows a board (it notices the
 * explorer's `data-page-fill` marker), and any other parent should be
 * `flex min-h-0 flex-1 flex-col`, itself in a column that fills the height. The list needs
 * nothing and scrolls with the page.
 */
export function IssueExplorer({
  workspace,
  project,
  person,
  defaultTeamId,
  creating: creatingFromPage,
  onCreatingChange,
}: {
  workspace: Workspace;
  project?: Project;
  person?: { assignee_id?: string; creator_id?: string };
  /**
   * The team new issues are filed in, and the board shows, while no team is picked (`?team=`):
   * a project's own team, say. Without one, the board shows the first team.
   */
  defaultTeamId?: string;
  /** The new-issue dialog, opened from the page's header instead of the explorer's toolbar. */
  creating?: boolean;
  onCreatingChange?: (creating: boolean) => void;
}) {
  // The team is part of the address, so the sidebar's team links and reloads land on it.
  const [params, setParams] = useSearchParams();
  const teamId = params.get('team') || ALL;
  const setTeamId = (id: string) => setParams(id === ALL ? {} : { team: id }, { replace: true });
  const [openOnly, setOpenOnly] = useState(true);
  const [labelId, setLabelId] = useState(ALL);
  const [cycleId, setCycleId] = useState(ALL);
  const [q, setQ] = useState('');
  const [ownCreating, setOwnCreating] = useState(false);
  const creating = creatingFromPage ?? ownCreating;
  const setCreating = onCreatingChange ?? setOwnCreating;
  // The state a board column's `+` files the new issue in.
  const [creatingIn, setCreatingIn] = useState<string | undefined>();
  const [view, setView] = useState<View>(rememberedView);
  const debouncedQ = useDebouncedValue(q.trim(), 300);
  const teamsResult = useQuery(teamsQuery(workspace.id));
  const teams = teamsResult.data ?? [];
  const { data: labels = [] } = useQuery(labelsQuery(workspace.id));
  // A label deleted elsewhere stops filtering.
  const activeLabel = labels.some((l) => l.id === labelId) ? labelId : ALL;
  // A team the caller cannot see (or that is gone) is no default.
  const fallbackTeam = teams.some((t) => t.id === defaultTeamId) ? defaultTeamId : undefined;
  // A board shows one team's workflow, so it needs a team.
  const boardTeam = teamId === ALL ? (fallbackTeam ?? teams[0]?.id) : teamId;
  const board = view === 'board';
  const showsBoard = board && boardTeam !== undefined;
  // The team on screen: the board's even while none is picked, otherwise the picked one (or all).
  const shownTeam = showsBoard ? boardTeam : teamId;
  // Cycles belong to a team, so the cycle filter exists only while one team is shown.
  const { data: teamCycles = [] } = useQuery({
    ...cyclesQuery(workspace.id, shownTeam),
    enabled: shownTeam !== ALL,
  });
  const activeCycle = shownTeam !== ALL && teamCycles.some((c) => c.id === cycleId) ? cycleId : ALL;
  const issuesResult = useQuery({
    ...issuesQuery(workspace.id, {
      team_id: teamId === ALL ? undefined : teamId,
      project_id: project?.id,
      label_id: activeLabel === ALL ? undefined : activeLabel,
      cycle_id: activeCycle === ALL ? undefined : activeCycle,
      assignee_id: person?.assignee_id,
      creator_id: person?.creator_id,
      open: openOnly,
      q: debouncedQ || undefined,
    }),
    // A changed filter keeps the last list (or board) on screen until its own list arrives,
    // instead of flashing the skeleton.
    placeholderData: keepPreviousData,
  });
  const issues = issuesResult.data;
  const isPending = issuesResult.isPending || teamsResult.isPending;
  // A list that could not be read is not an empty one. One that was read stays on screen when a
  // later refetch fails (the toast says so).
  const loadError =
    (teamsResult.data ? null : teamsResult.error) ??
    (issuesResult.data ? null : issuesResult.error);
  const retry = () => {
    if (teamsResult.error) void teamsResult.refetch();
    if (issuesResult.error) void issuesResult.refetch();
  };

  // `c` creates an issue, as long as the member is not typing somewhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const typing =
        target?.isContentEditable === true ||
        /^(INPUT|TEXTAREA|SELECT)$/.test(target?.tagName ?? '');
      if (e.key === 'c' && !typing && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        setCreating(true);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [setCreating]);
  // With no team picked every team's issues are read, and the board leaves out those of the
  // teams it does not show: a project's issues filed in other teams, say.
  // The last list, kept while another filter loads, says nothing about this board.
  const elsewhere =
    showsBoard && !issuesResult.isPlaceholderData
      ? (issues ?? []).filter((i) => i.team_id !== boardTeam)
      : [];
  const elsewhereTeams = teams
    .map((team) => ({ team, count: elsewhere.filter((i) => i.team_id === team.id).length }))
    .filter(({ count }) => count > 0);
  const teamOptions = teams.map((t) => ({ value: t.id, label: `${t.key} · ${t.name}` }));
  const chooseView = (next: View) => {
    setView(next);
    // With a default team the board shows it without narrowing the list to it.
    if (next === 'board' && teamId === ALL && !fallbackTeam && teams[0]) setTeamId(teams[0].id);
    try {
      localStorage.setItem(VIEW_KEY, next);
    } catch {
      // The choice lasts for this visit only.
    }
  };

  // One group per state name, ordered by what the state means and then by the workflow.
  const groups = new Map<string, Issue[]>();
  for (const issue of issues ?? []) {
    const group = groups.get(issue.state.name) ?? [];
    group.push(issue);
    groups.set(issue.state.name, group);
  }
  const ordered = [...groups.values()].sort((a, b) => {
    const [x, y] = [a[0]!.state, b[0]!.state];
    return (
      CATEGORY_ORDER.indexOf(x.category) - CATEGORY_ORDER.indexOf(y.category) ||
      x.position - y.position
    );
  });

  const newIssueButton = (
    <Button
      size="sm"
      onClick={() => setCreating(true)}
      disabled={teams.length === 0}
      aria-keyshortcuts="c"
    >
      <PlusIcon />
      New issue
      <kbd className="ml-1 hidden rounded border border-primary-foreground/30 px-1 font-mono text-[10px] sm:inline">
        C
      </kbd>
    </Button>
  );

  return (
    <ExplorerFrame embedded={Boolean(project || person)} board={board}>
      {!project && !person && (
        <PageHeader
          title="Issues"
          description={`Work tracked in ${workspace.name}. Open an issue and hand it to agents.`}
          actions={newIssueButton}
        />
      )}
      <div className="flex shrink-0 flex-wrap items-center gap-2">
        {/* A board is one team's: the select names the team it shows, and offers no "All teams". */}
        <OptionSelect
          value={shownTeam}
          onValueChange={setTeamId}
          options={showsBoard ? teamOptions : [{ value: ALL, label: 'All teams' }, ...teamOptions]}
          aria-label="Team"
          className="w-52"
        />
        {shownTeam !== ALL && teamCycles.length > 0 && (
          <OptionSelect
            value={activeCycle}
            onValueChange={setCycleId}
            options={[
              { value: ALL, label: 'Any cycle' },
              ...teamCycles.map((c) => ({
                value: c.id,
                label: c.status === 'active' ? `${cycleLabel(c)} · active` : cycleLabel(c),
              })),
            ]}
            aria-label="Cycle"
            className="w-44"
          />
        )}
        {labels.length > 0 && (
          <OptionSelect
            value={activeLabel}
            onValueChange={setLabelId}
            options={[
              { value: ALL, label: 'Any label' },
              ...labels.map((l) => ({
                value: l.id,
                label: (
                  <span className="flex items-center gap-2">
                    <span
                      aria-hidden
                      className="size-2 rounded-full"
                      style={{ backgroundColor: l.color }}
                    />
                    {l.name}
                  </span>
                ),
              })),
            ]}
            aria-label="Label"
            className="w-40"
          />
        )}
        <Input
          value={q}
          placeholder="Search title or ENG-12…"
          aria-label="Search issues"
          onChange={(e) => setQ(e.target.value)}
          className="w-56"
        />
        <label className="ml-auto flex items-center gap-2 text-sm text-muted-foreground">
          <Switch checked={openOnly} onCheckedChange={setOpenOnly} />
          Open only
        </label>
        {(project || person) && !onCreatingChange && newIssueButton}
        <div role="group" aria-label="View" className="flex rounded-md border p-0.5">
          <IconButton
            variant={view === 'list' ? 'secondary' : 'ghost'}
            label="List view"
            aria-pressed={view === 'list'}
            onClick={() => chooseView('list')}
          >
            <ListIcon />
          </IconButton>
          <IconButton
            variant={board ? 'secondary' : 'ghost'}
            label="Board view"
            aria-pressed={board}
            onClick={() => chooseView('board')}
          >
            <KanbanIcon />
          </IconButton>
        </div>
      </div>
      {loadError ? (
        <ErrorState title="Couldn’t load the issues" error={loadError} onRetry={retry} />
      ) : isPending ? (
        <IssueListSkeleton />
      ) : teams.length === 0 ? (
        <EmptyState
          icon={CircleDotIcon}
          title="Create a team first"
          description="Issues belong to a team. Add one under Teams, then come back."
          action={
            <Button asChild>
              <Link to="/app/settings/teams">
                <UsersIcon />
                Open Teams
              </Link>
            </Button>
          }
        />
      ) : showsBoard ? (
        <div className="flex min-h-0 flex-1 flex-col gap-2">
          {elsewhere.length > 0 && (
            <p className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 text-[13px] text-muted-foreground">
              {elsewhere.length === 1
                ? '1 issue here is in another team, not on this board.'
                : `${elsewhere.length} issues here are in other teams, not on this board.`}
              {elsewhereTeams.map(({ team, count }) => (
                <Button
                  key={team.id}
                  variant="link"
                  size="sm"
                  className="h-auto p-0 text-[13px]"
                  onClick={() => setTeamId(team.id)}
                >
                  Show {team.key} ({count})
                </Button>
              ))}
            </p>
          )}
          <IssueBoard
            key={boardTeam}
            workspace={workspace}
            teamId={boardTeam}
            issues={(issues ?? []).filter((i) => i.team_id === boardTeam)}
            hideClosed={openOnly}
            onCreate={(stateId) => {
              setCreatingIn(stateId);
              setCreating(true);
            }}
          />
        </div>
      ) : ordered.length === 0 ? (
        <EmptyState
          icon={CircleDotIcon}
          title="No issues here"
          description="Nothing matches these filters yet."
          action={<Button onClick={() => setCreating(true)}>Create an issue</Button>}
        />
      ) : (
        <div className="space-y-3 motion-safe:animate-fade-in">
          {ordered.map((group) => (
            <section key={group[0]!.state.name} className="overflow-hidden rounded-lg border">
              <h2 className="flex h-8 items-center gap-2 border-b bg-muted/40 px-3 text-[13px] font-medium">
                <StateGlyph category={group[0]!.state.category} color={group[0]!.state.color} />
                {group[0]!.state.name}
                <span className="text-xs font-normal text-muted-foreground">{group.length}</span>
              </h2>
              <ul className="divide-y">
                {group.map((issue) => (
                  <IssueRow key={issue.id} issue={issue} />
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
      <NewIssueDialog
        key={`${creating}:${teamId}:${creatingIn ?? ''}`}
        workspace={workspace}
        teams={teams}
        defaultTeam={board ? boardTeam : teamId === ALL ? fallbackTeam : teamId}
        defaultProject={project?.id}
        defaultState={creatingIn}
        open={creating}
        onClose={() => {
          setCreating(false);
          setCreatingIn(undefined);
        }}
      />
    </ExplorerFrame>
  );
}

/**
 * The explorer's frame: the page's own, or a plain column inside another page. On a board it is
 * a column that fills the height, and marks itself so a `PageContainer` around it does too.
 */
function ExplorerFrame({
  embedded,
  board,
  children,
}: {
  embedded: boolean;
  board: boolean;
  children: ReactNode;
}) {
  if (embedded) {
    return (
      <div
        data-page-fill={board || undefined}
        className={cn('w-full', board ? 'flex min-h-0 flex-1 flex-col gap-4' : 'space-y-4')}
      >
        {children}
      </div>
    );
  }
  return (
    <PageContainer className={board ? 'flex min-h-0 flex-1 flex-col' : undefined}>
      {children}
    </PageContainer>
  );
}

function IssuesWorkspace() {
  const { current } = useCurrentWorkspace();
  return current ? <IssueExplorer key={current.id} workspace={current} /> : <PageSkeleton />;
}

export default function IssuesPage() {
  const [params] = useSearchParams();
  // Old links (`?issue=<id>`) open the issue's own page.
  const legacy = legacyIssuePath(params);
  if (legacy) return <Navigate to={legacy} replace />;
  return (
    <>
      <Seo title="Issues" noIndex />
      <IssuesWorkspace />
    </>
  );
}
