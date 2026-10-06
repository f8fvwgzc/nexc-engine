import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CalendarIcon,
  ChevronRightIcon,
  CircleDotIcon,
  KanbanIcon,
  ListIcon,
  NetworkIcon,
  PlusIcon,
  Trash2Icon,
  WorkflowIcon,
} from 'lucide-react';
import { useEffect, useState, type ReactNode } from 'react';
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
  cyclesQuery,
  deleteIssue,
  issueQuery,
  issuesQuery,
  labelsQuery,
  projectsQuery,
  statesQuery,
  updateIssue,
} from '@/features/issues/api';
import { dueLabel, localToday } from '@/features/issues/due';
import { IssueTimeline } from '@/features/issues/issue-timeline';
import { LabelPicker } from '@/features/issues/label-picker';
import { SubIssues } from '@/features/issues/sub-issues';
import { membersQuery, teamsQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { cycleLabel } from '@/schemas/cycle';
import {
  PRIORITY_LABEL,
  type Issue,
  type IssueInput,
  type Project,
  type StateCategory,
} from '@/schemas/issue';
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
  defaultProject,
  open,
  onClose,
}: {
  workspace: Workspace;
  teams: Team[];
  defaultTeam: string | undefined;
  defaultProject: string | undefined;
  open: boolean;
  onClose: () => void;
}) {
  const refresh = useIssueRefresh();
  const [teamId, setTeamId] = useState(defaultTeam ?? teams[0]?.id ?? '');
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [priority, setPriority] = useState('0');
  const [projectId, setProjectId] = useState(defaultProject ?? NONE);
  const { data: projects = [] } = useQuery(projectsQuery(workspace.id));
  const create = useMutation({
    mutationFn: () =>
      createIssue(workspace.id, teamId, {
        title: title.trim(),
        description,
        priority: Number(priority),
        project_id: projectId === NONE ? undefined : projectId,
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
          <div className="grid grid-cols-3 gap-2">
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
            <OptionSelect
              value={projectId}
              onValueChange={setProjectId}
              options={[
                { value: NONE, label: 'No project' },
                ...projects.map((p) => ({ value: p.id, label: p.name })),
              ]}
              aria-label="Project"
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
  onOpen,
  onClose,
}: {
  workspace: Workspace;
  issue: Issue;
  /** Opens another issue in place of this one (its parent, a sub-issue). */
  onOpen: (issueId: string) => void;
  onClose: () => void;
}) {
  const refresh = useIssueRefresh();
  const navigate = useNavigate();
  const [title, setTitle] = useState(issue.title);
  const [description, setDescription] = useState(issue.description);
  const [confirming, setConfirming] = useState(false);
  const { data: states = [] } = useQuery(statesQuery(workspace.id, issue.team_id));
  const { data: projects = [] } = useQuery(projectsQuery(workspace.id));
  const { data: cycles = [] } = useQuery(cyclesQuery(workspace.id, issue.team_id));
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
  const { data: teams = [] } = useQuery(teamsQuery(workspace.id));
  const team = teams.find((t) => t.id === issue.team_id);
  // Text saves when the field is left, like every other property saves when it is picked.
  const commitText = () => {
    const next = title.trim();
    if (!next) {
      setTitle(issue.title);
      return;
    }
    if (next !== issue.title || description !== issue.description) {
      save.mutate({ title: next, description });
    }
  };

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="flex h-[85vh] max-h-[90vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-4xl">
        <DialogHeader className="flex-row items-center gap-2 border-b py-2 pr-12 pl-4">
          <DialogTitle className="flex min-w-0 flex-1 items-center gap-1.5 text-[13px] font-normal text-muted-foreground">
            <span className="truncate">{team?.name ?? 'Team'}</span>
            <ChevronRightIcon className="size-3.5 shrink-0" aria-hidden />
            {issue.parent && (
              <>
                <button
                  type="button"
                  className="truncate underline-offset-2 hover:text-foreground hover:underline"
                  onClick={() => onOpen(issue.parent!.id)}
                >
                  {issue.parent.identifier} {issue.parent.title}
                </button>
                <ChevronRightIcon className="size-3.5 shrink-0" aria-hidden />
              </>
            )}
            <span className="shrink-0 font-mono text-xs text-foreground">{issue.identifier}</span>
          </DialogTitle>
          <DialogDescription className="sr-only">
            {issue.title}, updated {formatRelative(issue.updated_at)}
          </DialogDescription>
          {issue.graph_id ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => void navigate(`/app/graphs/${issue.graph_id}`)}
            >
              <NetworkIcon />
              Open graph
            </Button>
          ) : (
            <Button
              variant="outline"
              size="sm"
              disabled={plan.isPending}
              onClick={() => plan.mutate()}
            >
              <WorkflowIcon />
              Plan as graph
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-muted-foreground hover:text-destructive"
            aria-label={`Delete ${issue.identifier}`}
            onClick={() => setConfirming(true)}
          >
            <Trash2Icon />
          </Button>
        </DialogHeader>
        <div className="grid min-h-0 flex-1 overflow-y-auto md:grid-cols-[minmax(0,1fr)_15rem] md:overflow-hidden">
          <div className="min-w-0 space-y-4 p-5 md:overflow-y-auto">
            <input
              value={title}
              maxLength={200}
              aria-label="Title"
              placeholder="Issue title"
              className="w-full bg-transparent text-lg font-semibold tracking-tight outline-none placeholder:text-muted-foreground"
              onChange={(e) => setTitle(e.target.value)}
              onBlur={commitText}
              onKeyDown={(e) => {
                if (e.key === 'Enter') e.currentTarget.blur();
              }}
            />
            <Textarea
              value={description}
              rows={5}
              aria-label="Description"
              placeholder="Add a description. It becomes the goal when the issue is planned as a graph."
              className="resize-none border-0 bg-transparent px-0 shadow-none focus-visible:ring-0 dark:bg-transparent"
              onChange={(e) => setDescription(e.target.value)}
              onBlur={commitText}
            />
            {(save.error ?? plan.error) && (
              <p role="alert" className="text-sm text-destructive">
                {errorMessage(save.error ?? plan.error)}
              </p>
            )}
            <SubIssues workspace={workspace} issue={issue} onOpen={onOpen} />
            <IssueTimeline
              issue={issue}
              canModerate={workspace.role === 'owner' || workspace.role === 'admin'}
            />
          </div>
          <aside
            aria-label="Properties"
            className="space-y-3 border-t bg-muted/20 p-4 md:overflow-y-auto md:border-t-0 md:border-l"
          >
            <Property label="Status">
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
                className="w-full"
              />
            </Property>
            <Property label="Priority">
              <OptionSelect
                value={String(issue.priority)}
                onValueChange={(p) => save.mutate({ priority: Number(p) })}
                options={PRIORITY_LABEL.map((label, value) => ({
                  value: String(value),
                  label: (
                    <span className="flex items-center gap-2">
                      <PriorityGlyph priority={value} />
                      {label}
                    </span>
                  ),
                }))}
                aria-label="Priority"
                className="w-full"
              />
            </Property>
            <Property label="Assignee">
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
                className="w-full"
              />
            </Property>
            <Property label="Labels">
              <LabelPicker
                workspace={workspace}
                selected={issue.labels}
                onChange={(label_ids) => save.mutate({ label_ids })}
              />
            </Property>
            <Property label="Project">
              <OptionSelect
                value={issue.project_id ?? NONE}
                onValueChange={(id) => save.mutate({ project_id: id === NONE ? null : id })}
                options={[
                  { value: NONE, label: 'No project' },
                  ...projects.map((p) => ({ value: p.id, label: p.name })),
                ]}
                aria-label="Project"
                className="w-full"
              />
            </Property>
            <Property label="Due date">
              <Input
                type="date"
                aria-label="Due date"
                value={issue.due_date ?? ''}
                className="h-8 w-full text-[13px]"
                onChange={(e) => {
                  const next = e.target.value || null;
                  if (next !== issue.due_date) save.mutate({ due_date: next });
                }}
              />
            </Property>
            {(cycles.length > 0 || issue.cycle) && (
              <Property label="Cycle">
                <OptionSelect
                  value={issue.cycle?.id ?? NONE}
                  onValueChange={(id) => save.mutate({ cycle_id: id === NONE ? null : id })}
                  options={[
                    { value: NONE, label: 'No cycle' },
                    ...cycles.map((c) => ({
                      value: c.id,
                      label: c.status === 'active' ? `${cycleLabel(c)} · active` : cycleLabel(c),
                    })),
                  ]}
                  aria-label="Cycle"
                  className="w-full"
                />
              </Property>
            )}
            <dl className="space-y-1 border-t pt-3 text-xs text-muted-foreground">
              <div className="flex justify-between gap-2">
                <dt>Created</dt>
                <dd>{formatRelative(issue.created_at)}</dd>
              </div>
              <div className="flex justify-between gap-2">
                <dt>Updated</dt>
                <dd>{formatRelative(issue.updated_at)}</dd>
              </div>
              {issue.completed_at && (
                <div className="flex justify-between gap-2">
                  <dt>Closed</dt>
                  <dd>{formatRelative(issue.completed_at)}</dd>
                </div>
              )}
            </dl>
          </aside>
        </div>
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

/** One property of an issue in the side panel: a small label over its control. */
function Property({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="space-y-1">
      <p className="text-xs text-muted-foreground">{label}</p>
      {children}
    </div>
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
        <span className="min-w-0 flex-1 truncate">
          {issue.title}
          {issue.parent && (
            <span className="ml-2 text-xs text-muted-foreground">
              <ChevronRightIcon className="mr-0.5 inline size-3" aria-hidden />
              {issue.parent.title}
            </span>
          )}
        </span>
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
          <NetworkIcon
            className="size-3.5 shrink-0 text-muted-foreground"
            aria-label="Has a graph"
          />
        )}
        {issue.due_date && <DueChip issue={issue} />}
        <span className="hidden shrink-0 text-xs text-muted-foreground tabular-nums sm:block">
          {formatRelative(issue.updated_at)}
        </span>
        <PersonGlyph name={issue.assignee?.name} />
      </button>
    </li>
  );
}

/** When an issue is due; red once that day has passed and the work is still open. */
function DueChip({ issue }: { issue: Issue }) {
  if (!issue.due_date) return null;
  const open = !['completed', 'canceled'].includes(issue.state.category);
  const { text, late } = dueLabel(issue.due_date, localToday(), open);
  return (
    <span
      title={`Due ${issue.due_date}`}
      className={`flex shrink-0 items-center gap-1 text-xs tabular-nums ${late ? 'font-medium text-destructive' : 'text-muted-foreground'}`}
    >
      <CalendarIcon aria-hidden className="size-3" />
      {text}
    </span>
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
                      {issue.due_date && <DueChip issue={issue} />}
                      {issue.sub_issues.total > 0 && (
                        <span
                          className="rounded-full border px-1.5 text-xs text-muted-foreground tabular-nums"
                          aria-label={`${issue.sub_issues.closed} of ${issue.sub_issues.total} sub-issues closed`}
                        >
                          {issue.sub_issues.closed}/{issue.sub_issues.total}
                        </span>
                      )}
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

/**
 * The issues of a workspace as a list grouped by state or as a team's board, with filters. Given
 * a `project` it shows only that project's issues and files new ones in it; given a `person` it
 * shows only what is assigned to, or was filed by, that member, under the caller's own heading.
 */
export function IssueExplorer({
  workspace,
  project,
  person,
}: {
  workspace: Workspace;
  project?: Project;
  person?: { assignee_id?: string; creator_id?: string };
}) {
  // The team is part of the address, so the sidebar's team links and reloads land on it.
  const [params, setParams] = useSearchParams();
  const teamId = params.get('team') ?? ALL;
  const setTeamId = (id: string) => setParams(id === ALL ? {} : { team: id }, { replace: true });
  const [openOnly, setOpenOnly] = useState(true);
  const [labelId, setLabelId] = useState(ALL);
  const [cycleId, setCycleId] = useState(ALL);
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
  // Cycles belong to a team, so the cycle filter exists only while one team is shown.
  const { data: teamCycles = [] } = useQuery({
    ...cyclesQuery(workspace.id, teamId),
    enabled: teamId !== ALL,
  });
  const activeCycle = teamId !== ALL && teamCycles.some((c) => c.id === cycleId) ? cycleId : ALL;
  const { data: issues, isPending } = useQuery(
    issuesQuery(workspace.id, {
      team_id: teamId === ALL ? undefined : teamId,
      project_id: project?.id,
      label_id: activeLabel === ALL ? undefined : activeLabel,
      cycle_id: activeCycle === ALL ? undefined : activeCycle,
      assignee_id: person?.assignee_id,
      creator_id: person?.creator_id,
      open: openOnly,
      q: debouncedQ || undefined,
    }),
  );
  const listed = issues?.find((i) => i.id === openId);
  // The opened issue is read again on its own: the list may be minutes old, and a linked
  // issue may be closed or filtered out of it. The list's copy shows until the fresh one lands.
  const { data: fresh } = useQuery({
    ...issueQuery(openId ?? ''),
    enabled: openId !== null,
    staleTime: 0,
  });
  const opened = (fresh?.id === openId ? fresh : undefined) ?? listed;
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

  const newIssueButton = (
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
  );

  return (
    <div
      className={
        project || person
          ? 'w-full space-y-4'
          : `mx-auto w-full space-y-5 p-4 sm:p-6 ${view === 'board' ? 'max-w-none' : 'max-w-5xl'}`
      }
    >
      {!project && !person && (
        <PageHeader
          title="Issues"
          description={`Work tracked in ${workspace.name}. Open an issue to plan and run it as a graph.`}
          actions={newIssueButton}
        />
      )}
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
        {teamId !== ALL && teamCycles.length > 0 && (
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
        {(project || person) && newIssueButton}
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
        defaultProject={project?.id}
        open={creating}
        onClose={() => setCreating(false)}
      />
      {opened && (
        <IssueDialog
          key={opened.id}
          workspace={workspace}
          issue={opened}
          onOpen={setOpenId}
          onClose={closeIssue}
        />
      )}
    </div>
  );
}

export default function IssuesPage() {
  const { current } = useCurrentWorkspace();
  return (
    <>
      <Seo title="Issues" noIndex />
      {current ? <IssueExplorer key={current.id} workspace={current} /> : <PageSkeleton />}
    </>
  );
}
