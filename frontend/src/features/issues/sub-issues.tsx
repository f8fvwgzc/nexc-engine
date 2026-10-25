import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ChevronRightIcon, CircleDashedIcon, PlusIcon, RotateCcwIcon } from 'lucide-react';
import { m } from 'motion/react';
import { useState } from 'react';
import { Link } from 'react-router-dom';

import { IconButton } from '@/components/custom-ui/icon-button';
import { PersonGlyph, PriorityGlyph, StateGlyph } from '@/components/custom-ui/issue-glyphs';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Progress } from '@/components/ui/progress';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';
import { MotionPresence, unfold } from '@/lib/motion';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import type { Issue, IssueState } from '@/schemas/issue';
import { runStatusSchema, type RunStatus } from '@/schemas/run';
import type { Workspace } from '@/schemas/workspace';

import {
  createIssue,
  issueQuery,
  issuesQuery,
  statesQuery,
  updateIssue,
  type IssueFilter,
} from './api';
import { SubIssueRun } from './issue-run';
import { issuePath } from './links';

/** What a sub-issue's row shows, as the issue list carries it. */
type Part = Pick<
  Issue,
  'id' | 'identifier' | 'title' | 'team_id' | 'state' | 'priority' | 'assignee' | 'created_at'
>;

/** A sub-issue being filed: shown at once, before the server answers. */
const DRAFT = 'draft:';
let drafts = 0;

const isClosed = (state: Pick<IssueState, 'category'>) =>
  state.category === 'completed' || state.category === 'canceled';

/**
 * The status of the latest run of a sub-issue's graph, when its list carries it (`run_status`);
 * null when it does not, or when the sub-issue has no graph or no run yet.
 */
function runStatusOf(part: object): RunStatus | null {
  const status = 'run_status' in part ? runStatusSchema.safeParse(part.run_status) : null;
  return status?.success ? status.data : null;
}

/** A sub-issue's status glyph, which opens a menu to move it to another state of its team. */
function PartStatus({
  workspace,
  part,
  onPick,
}: {
  workspace: Workspace;
  part: Part;
  onPick: (state: IssueState) => void;
}) {
  const [open, setOpen] = useState(false);
  const { data: states = [] } = useQuery({
    ...statesQuery(workspace.id, part.team_id),
    enabled: open,
  });
  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <DropdownMenuTrigger asChild>
        <IconButton
          label={`Status of ${part.identifier}: ${part.state.name}`}
          size="icon-xs"
          className="relative z-10 -ml-1 shrink-0"
        >
          <StateGlyph category={part.state.category} color={part.state.color} />
        </IconButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-44">
        <DropdownMenuLabel>Change status</DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={part.state.id}
          onValueChange={(id) => {
            const next = states.find((s) => s.id === id);
            if (next && next.id !== part.state.id) onPick(next);
          }}
        >
          {(states.length > 0 ? states : [part.state]).map((state) => (
            <DropdownMenuRadioItem key={state.id} value={state.id} className="gap-2">
              <StateGlyph category={state.category} color={state.color} />
              {state.name}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * The parts an issue is split into, with how far along they are, where the agents' latest run of
 * each stands, and a quiet last row that turns into a field to add one (Enter files it and keeps
 * the field for the next, Escape closes it). A part is moved to another state from its glyph,
 * and shows at once, before the server answers; rows slide in and out. A new part is filed with
 * the same team as the issue.
 */
export function SubIssues({ workspace, issue }: { workspace: Workspace; issue: Issue }) {
  const queryClient = useQueryClient();
  const [title, setTitle] = useState('');
  const [adding, setAdding] = useState(false);
  const [folded, setFolded] = useState(false);
  // The row of a part filed here keeps the draft's key, so it does not slide in a second time.
  const [keys, setKeys] = useState<Record<string, string>>({});
  const list = issuesQuery(workspace.id, { parent_id: issue.id });
  const parts = useQuery({ ...list, enabled: issue.sub_issues.total > 0 });
  // In the order they were added, not by last change.
  const rows: Part[] = [...(parts.data ?? [])].sort((a, b) =>
    a.created_at.localeCompare(b.created_at),
  );
  const { total, closed } = issue.sub_issues;

  /** Moves the issue's count of parts (and of closed ones) as a part is added or moved. */
  const count = (more: number, moreClosed: number) =>
    queryClient.setQueryData<Issue>(qk.issues.one(issue.id), (prev) =>
      prev
        ? {
            ...prev,
            sub_issues: {
              total: prev.sub_issues.total + more,
              closed: prev.sub_issues.closed + moreClosed,
            },
          }
        : prev,
    );
  /** What changes elsewhere when a part moves: the lists, and what counts or reports it. */
  const refresh = (part: { id: string }) => {
    void queryClient.invalidateQueries({ queryKey: qk.issues.lists(workspace.id) });
    void queryClient.invalidateQueries({ queryKey: qk.issues.events(part.id) });
    void queryClient.invalidateQueries({ queryKey: qk.issues.inbox(workspace.id) });
    for (const queryKey of [
      qk.issues.projectLists(workspace.id),
      qk.issues.cycles(workspace.id, issue.team_id),
    ]) {
      void queryClient.invalidateQueries({ queryKey, refetchType: 'none' });
    }
  };

  const filing = ['issues', 'sub-issue', issue.id];
  const add = useMutation({
    mutationKey: filing,
    mutationFn: (draft: { key: string; title: string }) =>
      createIssue(workspace.id, issue.team_id, { title: draft.title, parent_id: issue.id }),
    meta: { errorToast: false },
    onMutate: async (draft) => {
      // A read already on its way would answer without the draft and drop it.
      await queryClient.cancelQueries({ queryKey: list.queryKey });
      const row: Part = {
        id: `${DRAFT}${draft.key}`,
        identifier: '',
        title: draft.title,
        team_id: issue.team_id,
        state: issue.state,
        priority: 0,
        assignee: null,
        created_at: new Date().toISOString(),
      };
      queryClient.setQueryData<object[]>(list.queryKey, (prev) => [...(prev ?? []), row]);
    },
    onError: (_error, draft) => {
      queryClient.setQueryData<Part[]>(list.queryKey, (prev) =>
        prev?.filter((row) => row.id !== `${DRAFT}${draft.key}`),
      );
      setTitle((current) => current || draft.title);
    },
    onSuccess: (created, draft) => {
      const draftId = `${DRAFT}${draft.key}`;
      setKeys((current) => ({ ...current, [created.id]: draftId }));
      queryClient.setQueryData<object[]>(list.queryKey, (prev) => {
        const rows = (prev ?? []) as Part[];
        return rows.some((row) => row.id === draftId)
          ? rows.map((row) => (row.id === draftId ? created : row))
          : [...rows, created];
      });
      count(1, isClosed(created.state) ? 1 : 0);
      // While more parts are being filed, their drafts stay: the list is read once they land.
      const more = queryClient.isMutating({ mutationKey: filing }) > 1;
      void queryClient.invalidateQueries({
        queryKey: qk.issues.lists(workspace.id),
        predicate: (query) =>
          !more || (query.queryKey[3] as IssueFilter | undefined)?.parent_id !== issue.id,
      });
    },
  });

  const move = useMutation({
    mutationFn: ({ part, state }: { part: Part; state: IssueState }) =>
      updateIssue(part.id, { state_id: state.id }),
    onMutate: async ({ part, state }) => {
      await queryClient.cancelQueries({ queryKey: list.queryKey });
      queryClient.setQueryData(list.queryKey, (prev) =>
        prev?.map((row) => (row.id === part.id ? { ...row, state } : row)),
      );
      count(0, Number(isClosed(state)) - Number(isClosed(part.state)));
    },
    onError: (_error, { part, state }) => {
      queryClient.setQueryData(list.queryKey, (prev) =>
        prev?.map((row) => (row.id === part.id ? { ...row, state: part.state } : row)),
      );
      count(0, Number(isClosed(part.state)) - Number(isClosed(state)));
    },
    onSuccess: (updated) => {
      // The part's own page, if it was read, shows the answer; nothing reads it again here.
      queryClient.setQueryData(qk.issues.one(updated.id), updated);
      refresh(updated);
    },
  });

  const warm = (id: string) => void queryClient.prefetchQuery(issueQuery(id));

  const file = () => {
    const text = title.trim();
    if (!text) return;
    drafts += 1;
    add.mutate({ key: String(drafts), title: text });
    setTitle('');
  };

  let body;
  if (total > 0 && parts.isPending) {
    body = (
      <div role="status" aria-label="Loading the sub-issues" className="space-y-1">
        {Array.from({ length: Math.min(total, 3) }, (_, i) => (
          <Skeleton key={i} className="h-8 w-full" />
        ))}
      </div>
    );
  } else if (total > 0 && parts.data === undefined) {
    body = (
      <div role="alert" className="flex items-center gap-2 text-sm text-muted-foreground">
        <span className="flex-1">Couldn’t load the sub-issues.</span>
        <Button variant="ghost" size="sm" onClick={() => void parts.refetch()}>
          <RotateCcwIcon />
          Try again
        </Button>
      </div>
    );
  } else {
    body = (
      <ul className="-mx-2">
        <MotionPresence>
          {rows.map((part) => {
            const pending = part.id.startsWith(DRAFT);
            const done = isClosed(part.state);
            return (
              <m.li key={keys[part.id] ?? part.id} {...unfold} className="overflow-hidden">
                {pending ? (
                  <div className="flex h-8 items-center gap-2 px-2 text-[13px] text-muted-foreground opacity-60">
                    <CircleDashedIcon aria-hidden className="size-3.5 shrink-0" />
                    <span className="min-w-0 flex-1 truncate">{part.title}</span>
                    <span className="text-xs">Adding…</span>
                  </div>
                ) : (
                  <div
                    className={cn(
                      'relative flex h-8 items-center gap-2 rounded-md px-2 text-[13px] transition-colors hover:bg-muted/50',
                      done && 'text-muted-foreground',
                    )}
                  >
                    <PartStatus
                      workspace={workspace}
                      part={part}
                      onPick={(state) => move.mutate({ part, state })}
                    />
                    <Link
                      to={issuePath(part.id)}
                      title={`${part.identifier} ${part.title}`}
                      // Pointed at or focused, the part's page starts loading: it opens whole.
                      onPointerEnter={() => warm(part.id)}
                      onFocus={() => warm(part.id)}
                      className="flex min-w-0 flex-1 items-center gap-2 outline-none after:absolute after:inset-0 after:rounded-md focus-visible:after:ring-2 focus-visible:after:ring-ring/50 focus-visible:after:ring-inset"
                    >
                      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
                        {part.identifier}
                      </span>
                      <span className="truncate">{part.title}</span>
                    </Link>
                    <SubIssueRun status={runStatusOf(part)} />
                    <PriorityGlyph priority={part.priority} />
                    <PersonGlyph name={part.assignee?.name} />
                  </div>
                )}
              </m.li>
            );
          })}
        </MotionPresence>
        <li>
          {adding ? (
            <div className="flex h-8 items-center gap-2 rounded-md bg-muted/40 px-2">
              <CircleDashedIcon aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
              <input
                autoFocus
                value={title}
                maxLength={200}
                placeholder="Sub-issue title"
                aria-label="Sub-issue title"
                className="h-full min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-muted-foreground"
                onChange={(e) => setTitle(e.target.value)}
                onBlur={() => {
                  if (!title.trim()) setAdding(false);
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    file();
                  } else if (e.key === 'Escape') {
                    e.stopPropagation();
                    setTitle('');
                    setAdding(false);
                  }
                }}
              />
            </div>
          ) : (
            <Button
              variant="ghost"
              size="sm"
              className="h-8 w-full justify-start gap-2 px-2 text-[13px] font-normal text-muted-foreground hover:bg-muted/50"
              onClick={() => setAdding(true)}
            >
              <PlusIcon />
              Add sub-issue
            </Button>
          )}
        </li>
      </ul>
    );
  }

  return (
    <section aria-label="Sub-issues" className="space-y-2">
      <div className="flex items-center gap-2">
        <h2 className="min-w-0">
          <button
            type="button"
            aria-expanded={!folded}
            onClick={() => setFolded(!folded)}
            className="flex items-center gap-1.5 rounded-sm text-[13px] font-medium text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            <ChevronRightIcon
              aria-hidden
              className={cn('size-3.5 transition-transform', !folded && 'rotate-90')}
            />
            Sub-issues
          </button>
        </h2>
        {total > 0 && (
          <span className="flex items-center gap-2 text-xs text-muted-foreground tabular-nums">
            <Progress
              value={(closed / total) * 100}
              aria-label={`${closed} of ${total} sub-issues closed`}
              className="h-1 w-20"
            />
            {closed}/{total}
          </span>
        )}
        <IconButton
          label="New sub-issue"
          size="icon-xs"
          className="ml-auto text-muted-foreground hover:text-foreground"
          onClick={() => {
            setFolded(false);
            setAdding(true);
          }}
        >
          <PlusIcon />
        </IconButton>
      </div>
      <MotionPresence>
        {!folded && (
          <m.div key="parts" {...unfold} className="overflow-hidden">
            {body}
          </m.div>
        )}
      </MotionPresence>
      {(add.error ?? move.error) && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(add.error ?? move.error)}
        </p>
      )}
    </section>
  );
}
