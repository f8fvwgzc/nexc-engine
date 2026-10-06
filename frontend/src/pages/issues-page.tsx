import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CircleDotIcon,
  KanbanIcon,
  ListIcon,
  NetworkIcon,
  PlusIcon,
  Trash2Icon,
  WorkflowIcon,
} from 'lucide-react';
import { useEffect, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import {
  LabelChip,
  PersonGlyph,
  PriorityGlyph,
  StateGlyph,
} from '@/components/custom-ui/issue-glyphs';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import {
  createIssue,
  createIssueGraph,
  deleteIssue,
  issueQuery,
  issuesQuery,
  labelsQuery,
  projectsQuery,
  statesQuery,
  updateIssue,
} from '@/features/issues/api';
import { IssueTimeline } from '@/features/issues/issue-timeline';
import { LabelPicker } from '@/features/issues/label-picker';
import { membersQuery, teamsQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { PRIORITY_LABEL, type Issue, type IssueInput, type StateCategory } from '@/schemas/issue';
import type { Team, Workspace } from '@/schemas/workspace';

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
const NONE = '__none__';

/** Order in which groups of states are listed: what is being worked on first. */
const CATEGORY_ORDER: StateCategory[] = [
  'started',
  'unstarted',
  'backlog',
  'completed',
  'canceled',
];

const PRIORITY_OPTIONS = PRIORITY_LABEL.map((label, value) => ({ value: String(value), label }));

function useIssueRefresh() {
  const queryClient = useQueryClient();
  return () => queryClient.invalidateQueries({ queryKey: qk.issues.all });
}

function NewIssueDialog({
  workspace,
  teams,
  defaultTeam,
  open,
  onClose,
}: {
  workspace: Workspace;
  teams: Team[];
  defaultTeam: string | undefined;
  open: boolean;
  onClose: () => void;
}) {
  const refresh = useIssueRefresh();
  const [teamId, setTeamId] = useState(defaultTeam ?? teams[0]?.id ?? '');
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [priority, setPriority] = useState('0');
  const create = useMutation({
    mutationFn: () =>
      createIssue(workspace.id, teamId, {
        title: title.trim(),
        description,
        priority: Number(priority),
      }),
    meta: { errorToast: false, successMessage: 'Issue created' },
    onSuccess: () => {
      void refresh();
      setTitle('');
      setDescription('');
      setPriority('0');
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (title.trim() && teamId) create.mutate();
          }}
        >
          <DialogHeader>
            <DialogTitle>New issue</DialogTitle>
            <DialogDescription>
              It gets the team’s next number and starts in the team’s first open state.
            </DialogDescription>
          </DialogHeader>
          <div className="grid grid-cols-2 gap-2">
            <OptionSelect
              value={teamId}
              onValueChange={setTeamId}
              options={teams.map((t) => ({ value: t.id, label: `${t.key} · ${t.name}` }))}
              aria-label="Team"
            />
            <OptionSelect
              value={priority}
              onValueChange={setPriority}
              options={PRIORITY_OPTIONS}
              aria-label="Priority"
            />
          </div>
          <Input
            autoFocus
            value={title}
            maxLength={200}
            placeholder="Issue title"
            aria-label="Title"
            onChange={(e) => setTitle(e.target.value)}
          />
          <Textarea
            value={description}
            rows={5}
            placeholder="What needs to happen, and why (optional). This becomes the goal when the issue is planned as a graph."
            aria-label="Description"
            onChange={(e) => setDescription(e.target.value)}
          />
          {create.error && (
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(create.error)}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!title.trim() || !teamId || create.isPending}>
              Create issue
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Everything about one issue, editable in place. */
function IssueDialog({
  workspace,
  issue,
  onClose,
}: {
  workspace: Workspace;
  issue: Issue;
  onClose: () => void;
}) {
  const refresh = useIssueRefresh();
  const navigate = useNavigate();
  const [title, setTitle] = useState(issue.title);
  const [description, setDescription] = useState(issue.description);
  const [confirming, setConfirming] = useState(false);
  const { data: states = [] } = useQuery(statesQuery(workspace.id, issue.team_id));
  const { data: projects = [] } = useQuery(projectsQuery(workspace.id));
  // Guests cannot list the workspace's members; they keep whoever is assigned.
  const { data: members = [] } = useQuery({
    ...membersQuery(workspace.id),
    enabled: workspace.role !== 'guest',
  });
  const save = useMutation({
    mutationFn: (body: Partial<IssueInput>) => updateIssue(issue.id, body),
    onSuccess: refresh,
  });
  const plan = useMutation({
    mutationFn: () => createIssueGraph(issue.id),
    meta: { successMessage: 'Graph created for this issue' },
    onSuccess: (linked) => {
      void refresh();
      if (linked.graph_id) void navigate(`/app/graphs/${linked.graph_id}`);
    },
  });
  const remove = useMutation({
    mutationFn: () => deleteIssue(issue.id),
    meta: { successMessage: 'Issue deleted' },
    onSuccess: () => {
      void refresh();
      onClose();
    },
  });
  const textDirty = title.trim() !== issue.title || description !== issue.description;

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Badge variant="outline" className="font-mono">
              {issue.identifier}
            </Badge>
            <span className="truncate">{issue.title}</span>
          </DialogTitle>
          <DialogDescription>
            Updated {formatRelative(issue.updated_at)}
            {issue.completed_at ? ` · closed ${formatRelative(issue.completed_at)}` : ''}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <Input
            value={title}
            maxLength={200}
            aria-label="Title"
            onChange={(e) => setTitle(e.target.value)}
          />
          <Textarea
            value={description}
            rows={6}
            aria-label="Description"
            placeholder="Describe the work. This is the goal of the issue’s graph."
            onChange={(e) => setDescription(e.target.value)}
          />
          <div className="grid gap-2 sm:grid-cols-2">
            <OptionSelect
              value={issue.state.id}
              onValueChange={(state_id) => save.mutate({ state_id })}
              options={states.map((s) => ({
                value: s.id,
                label: (
                  <span className="flex items-center gap-2">
                    <StateGlyph category={s.category} color={s.color} />
                    {s.name}
                  </span>
                ),
              }))}
              aria-label="State"
            />
            <OptionSelect
              value={String(issue.priority)}
              onValueChange={(p) => save.mutate({ priority: Number(p) })}
              options={PRIORITY_OPTIONS}
              aria-label="Priority"
            />
            <OptionSelect
              value={issue.assignee?.user_id ?? NONE}
              onValueChange={(id) => save.mutate({ assignee_id: id === NONE ? null : id })}
              options={[
                { value: NONE, label: 'Unassigned' },
                ...(issue.assignee && !members.some((m) => m.user_id === issue.assignee?.user_id)
                  ? [{ value: issue.assignee.user_id, label: issue.assignee.name }]
                  : []),
                ...members.map((m) => ({ value: m.user_id, label: m.name })),
              ]}
              aria-label="Assignee"
            />
            <OptionSelect
              value={issue.project_id ?? NONE}
              onValueChange={(id) => save.mutate({ project_id: id === NONE ? null : id })}
              options={[
                { value: NONE, label: 'No project' },
                ...projects.map((p) => ({ value: p.id, label: p.name })),
              ]}
              aria-label="Project"
            />
          </div>
          <LabelPicker
            workspace={workspace}
            selected={issue.labels}
            onChange={(label_ids) => save.mutate({ label_ids })}
          />
          {(save.error ?? plan.error) && (
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(save.error ?? plan.error)}
            </p>
          )}
        </div>
        <DialogFooter className="sm:justify-between">
          <Button variant="ghost" className="text-destructive" onClick={() => setConfirming(true)}>
            <Trash2Icon />
            Delete
          </Button>
          <div className="flex flex-wrap gap-2">
            {issue.graph_id ? (
              <Button
                variant="outline"
                onClick={() => void navigate(`/app/graphs/${issue.graph_id}`)}
              >
                <NetworkIcon />
                Open graph
              </Button>
            ) : (
              <Button variant="outline" disabled={plan.isPending} onClick={() => plan.mutate()}>
                <WorkflowIcon />
                Plan as graph
              </Button>
            )}
            <Button
              disabled={!textDirty || !title.trim() || save.isPending}
              onClick={() => save.mutate({ title: title.trim(), description })}
            >
              Save
            </Button>
          </div>
        </DialogFooter>
        <IssueTimeline
          issue={issue}
          canModerate={workspace.role === 'owner' || workspace.role === 'admin'}
        />
        <ConfirmDialog
          open={confirming}
          onOpenChange={setConfirming}
          title={`Delete ${issue.identifier}?`}
          description="The issue is removed. A graph created for it stays."
          confirmLabel="Delete issue"
          destructive
          onConfirm={() => remove.mutate()}
        />
      </DialogContent>
    </Dialog>
  );
}

function IssueRow({ issue, onOpen }: { issue: Issue; onOpen: () => void }) {
  return (
    <li>
      <button
        type="button"
        onClick={onOpen}
        className="flex h-9 w-full items-center gap-2.5 px-3 text-left text-[13px] transition-colors outline-none hover:bg-muted/60 focus-visible:bg-muted/60"
      >
        <PriorityGlyph priority={issue.priority} />
        <span className="w-16 shrink-0 font-mono text-xs text-muted-foreground">
          {issue.identifier}
        </span>
        <StateGlyph category={issue.state.category} color={issue.state.color} />
        <span className="min-w-0 flex-1 truncate">{issue.title}</span>
        <span className="hidden max-w-[40%] shrink-0 items-center gap-1 overflow-hidden md:flex">
          {issue.labels.map((label) => (
            <LabelChip key={label.id} name={label.name} color={label.color} />
          ))}
        </span>
        {issue.graph_id && (
          <NetworkIcon
            className="size-3.5 shrink-0 text-muted-foreground"
            aria-label="Has a graph"
          />
        )}
        <span className="hidden shrink-0 text-xs text-muted-foreground tabular-nums sm:block">
          {formatRelative(issue.updated_at)}
        </span>
        <PersonGlyph name={issue.assignee?.name} />
      </button>
    </li>
  );
}

/** Rows shaped like the list, shown while it loads. */
function IssueListSkeleton() {
  return (
    <div className="overflow-hidden rounded-lg border" aria-busy aria-label="Loading issues">
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
 * One column per state of the team's workflow. Dragging a card to another column moves the issue
 * to that state; opening a card offers the same change without a pointer.
 */
function IssueBoard({
  workspace,
  teamId,
  issues,
  hideClosed,
  onOpen,
}: {
  workspace: Workspace;
  teamId: string;
  issues: Issue[];
  hideClosed: boolean;
  onOpen: (id: string) => void;
}) {
  const refresh = useIssueRefresh();
  const { data: states = [] } = useQuery(statesQuery(workspace.id, teamId));
  const [over, setOver] = useState<string | null>(null);
  const move = useMutation({
    mutationFn: ({ id, state_id }: { id: string; state_id: string }) =>
      updateIssue(id, { state_id }),
    onSettled: refresh,
  });
  const columns = states.filter(
    (s) => !hideClosed || (s.category !== 'completed' && s.category !== 'canceled'),
  );
  return (
    <div className="flex gap-3 overflow-x-auto pb-2">
      {columns.map((state) => {
        const cards = issues.filter((i) => i.state.id === state.id);
        return (
          <section
            key={state.id}
            aria-label={state.name}
            data-over={over === state.id}
            className="flex w-72 shrink-0 flex-col rounded-xl border bg-muted/30 transition-colors data-[over=true]:border-ring data-[over=true]:bg-muted/60"
            onDragOver={(e) => {
              e.preventDefault();
              e.dataTransfer.dropEffect = 'move';
              setOver(state.id);
            }}
            onDragLeave={() => setOver((current) => (current === state.id ? null : current))}
            onDrop={(e) => {
              e.preventDefault();
              setOver(null);
              const id = e.dataTransfer.getData('text/plain');
              const issue = issues.find((i) => i.id === id);
              if (issue && issue.state.id !== state.id) move.mutate({ id, state_id: state.id });
            }}
          >
            <h2 className="flex items-center gap-2 px-3 py-2 text-[13px] font-medium">
              <StateGlyph category={state.category} color={state.color} />
              {state.name}
              <span className="text-xs font-normal text-muted-foreground">{cards.length}</span>
            </h2>
            <ul className="flex min-h-16 flex-1 flex-col gap-2 px-2 pb-2">
              {cards.map((issue) => (
                <li key={issue.id}>
                  <button
                    type="button"
                    draggable
                    onDragStart={(e) => {
                      e.dataTransfer.setData('text/plain', issue.id);
                      e.dataTransfer.effectAllowed = 'move';
                    }}
                    onClick={() => onOpen(issue.id)}
                    className="w-full cursor-grab space-y-1.5 rounded-lg border bg-background p-2.5 text-left text-sm shadow-xs outline-none hover:border-ring/60 focus-visible:border-ring active:cursor-grabbing motion-safe:animate-fade-in"
                  >
                    <span className="flex items-center gap-2 font-mono text-xs text-muted-foreground">
                      {issue.identifier}
                      {issue.graph_id && (
                        <NetworkIcon className="size-3" aria-label="Has a graph" />
                      )}
                      <PersonGlyph name={issue.assignee?.name} className="ml-auto font-sans" />
                    </span>
                    <span className="line-clamp-2 text-[13px]">{issue.title}</span>
                    <span className="flex flex-wrap items-center gap-1">
                      <PriorityGlyph priority={issue.priority} />
                      {issue.labels.map((label) => (
                        <LabelChip key={label.id} name={label.name} color={label.color} />
                      ))}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        );
      })}
    </div>
  );
}

function Issues({ workspace }: { workspace: Workspace }) {
  // The team is part of the address, so the sidebar's team links and reloads land on it.
  const [params, setParams] = useSearchParams();
  const teamId = params.get('team') ?? ALL;
  const setTeamId = (id: string) => setParams(id === ALL ? {} : { team: id }, { replace: true });
  const [openOnly, setOpenOnly] = useState(true);
  const [labelId, setLabelId] = useState(ALL);
  const [q, setQ] = useState('');
  const [creating, setCreating] = useState(false);
  // An issue can be opened by address (`?issue=<id>`), as the inbox does.
  const [openId, setOpenId] = useState<string | null>(() => params.get('issue'));
  const [view, setView] = useState<View>(rememberedView);
  const debouncedQ = useDebouncedValue(q.trim(), 300);
  const { data: teams = [] } = useQuery(teamsQuery(workspace.id));
  const { data: labels = [] } = useQuery(labelsQuery(workspace.id));
  // A label deleted elsewhere stops filtering.
  const activeLabel = labels.some((l) => l.id === labelId) ? labelId : ALL;
  const { data: issues, isPending } = useQuery(
    issuesQuery(workspace.id, {
      team_id: teamId === ALL ? undefined : teamId,
      label_id: activeLabel === ALL ? undefined : activeLabel,
      open: openOnly,
      q: debouncedQ || undefined,
    }),
  );
  const listed = issues?.find((i) => i.id === openId);
  // A linked issue may be closed or filtered out of the list; it is fetched on its own.
  const { data: linked } = useQuery({
    ...issueQuery(openId ?? ''),
    enabled: openId !== null && issues !== undefined && !listed,
  });
  const opened = listed ?? (linked?.id === openId ? linked : undefined);
  const closeIssue = () => {
    setOpenId(null);
    if (params.has('issue')) {
      const next = new URLSearchParams(params);
      next.delete('issue');
      setParams(next, { replace: true });
    }
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
  }, []);
  // A board shows one team's workflow, so it needs a team.
  const boardTeam = teamId === ALL ? teams[0]?.id : teamId;
  const chooseView = (next: View) => {
    setView(next);
    if (next === 'board' && teamId === ALL && teams[0]) setTeamId(teams[0].id);
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

  return (
    <div
      className={`mx-auto w-full space-y-5 p-4 sm:p-6 ${view === 'board' ? 'max-w-none' : 'max-w-5xl'}`}
    >
      <PageHeader
        title="Issues"
        description={`Work tracked in ${workspace.name}. Open an issue to plan and run it as a graph.`}
        actions={
          <Button
            size="sm"
            onClick={() => setCreating(true)}
            disabled={teams.length === 0}
            aria-keyshortcuts="c"
          >
            <PlusIcon />
            New issue
            <kbd className="ml-1 rounded border border-primary-foreground/30 px-1 font-mono text-[10px]">
              C
            </kbd>
          </Button>
        }
      />
      <div className="flex flex-wrap items-center gap-2">
        <OptionSelect
          value={teamId}
          onValueChange={setTeamId}
          options={[
            { value: ALL, label: 'All teams' },
            ...teams.map((t) => ({ value: t.id, label: `${t.key} · ${t.name}` })),
          ]}
          aria-label="Team"
          className="w-52"
        />
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
        <div role="group" aria-label="View" className="flex rounded-md border p-0.5">
          <Button
            variant={view === 'list' ? 'secondary' : 'ghost'}
            size="icon-sm"
            aria-label="List view"
            aria-pressed={view === 'list'}
            onClick={() => chooseView('list')}
          >
            <ListIcon />
          </Button>
          <Button
            variant={view === 'board' ? 'secondary' : 'ghost'}
            size="icon-sm"
            aria-label="Board view"
            aria-pressed={view === 'board'}
            onClick={() => chooseView('board')}
          >
            <KanbanIcon />
          </Button>
        </div>
      </div>
      {isPending ? (
        <IssueListSkeleton />
      ) : teams.length === 0 ? (
        <EmptyState
          icon={CircleDotIcon}
          title="Create a team first"
          description="Issues belong to a team. Add one under Teams, then come back."
        />
      ) : view === 'board' && boardTeam ? (
        <IssueBoard
          workspace={workspace}
          teamId={boardTeam}
          issues={(issues ?? []).filter((i) => i.team_id === boardTeam)}
          hideClosed={openOnly}
          onOpen={setOpenId}
        />
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
                  <IssueRow key={issue.id} issue={issue} onOpen={() => setOpenId(issue.id)} />
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
      <NewIssueDialog
        key={`${creating}:${teamId}`}
        workspace={workspace}
        teams={teams}
        defaultTeam={teamId === ALL ? undefined : teamId}
        open={creating}
        onClose={() => setCreating(false)}
      />
      {opened && (
        <IssueDialog key={opened.id} workspace={workspace} issue={opened} onClose={closeIssue} />
      )}
    </div>
  );
}

export default function IssuesPage() {
  const { current } = useCurrentWorkspace();
  return (
    <>
      <Seo title="Issues" noIndex />
      {current ? <Issues key={current.id} workspace={current} /> : <PageSkeleton />}
    </>
  );
}
