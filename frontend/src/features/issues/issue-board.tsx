import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { FoldHorizontalIcon, NetworkIcon, PlusIcon } from 'lucide-react';
import { useRef, useState, type DragEvent, type MouseEvent } from 'react';
import { Link } from 'react-router-dom';

import { GithubChip } from '@/components/custom-ui/github-chip';
import { IconButton } from '@/components/custom-ui/icon-button';
import {
  LabelChip,
  PersonGlyph,
  PriorityGlyph,
  StateGlyph,
} from '@/components/custom-ui/issue-glyphs';
import { ScrollRegion } from '@/components/custom-ui/scroll-region';
import { onIntent, prefetchIssue } from '@/lib/prefetch';
import { qk } from '@/lib/query-keys';
import type { IssueState, IssueSummary } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import { statesQuery, updateIssue } from './api';
import { useFoldedColumns } from './board-prefs';
import { DueChip } from './due-chip';
import { issueLinkTitle, issuePath } from './links';

/**
 * One column per state of the team's workflow, as tall as the space the board is given: each
 * column scrolls on its own and the board scrolls sideways. Dragging a card to another column, or
 * onto a folded one, moves the issue to that state; the issue's page offers the same change
 * without a pointer. A column's `+` files a new issue in that state, and a column folds to a
 * narrow strip (remembered per team) that unfolds on a click.
 *
 * The board takes the height left in a flex column (`flex-1`); key it by `teamId`.
 */
export function IssueBoard({
  workspace,
  teamId,
  issues,
  hideClosed,
  onCreate,
}: {
  workspace: Workspace;
  teamId: string;
  issues: IssueSummary[];
  hideClosed: boolean;
  /** Opens the new-issue dialog in this team, starting in the given state. */
  onCreate: (stateId: string) => void;
}) {
  const queryClient = useQueryClient();
  const { data: states = [] } = useQuery(statesQuery(workspace.id, teamId));
  const [folded, toggleFolded] = useFoldedColumns(teamId);
  const [over, setOver] = useState<string | null>(null);
  // The column just folded or unfolded from the keyboard: its new button takes the focus the old
  // one had. A click from the keyboard (Enter or Space) has `detail` 0; a pointer's click leaves
  // the focus be, so no tooltip pops up where the pointer is not.
  const refocus = useRef<string | null>(null);
  const toggle = (stateId: string, e: MouseEvent) => {
    refocus.current = e.detail === 0 ? stateId : null;
    toggleFolded(stateId);
  };
  const focusIfToggled = (stateId: string) => (element: HTMLButtonElement | null) => {
    if (element && refocus.current === stateId) {
      refocus.current = null;
      element.focus();
    }
  };
  // Every issue list of the workspace, whatever it is filtered by.
  const lists = [...qk.issues.all, workspace.id, 'list'] as const;
  const move = useMutation({
    mutationFn: ({ id, state_id }: { id: string; state_id: string }) =>
      updateIssue(id, { state_id }),
    // The card lands in its new column at once; the lists are read again either way, and put
    // back as they were if the move fails. A read still in flight is dropped first, so it
    // cannot put the card back in its old column.
    onMutate: async ({ id, state_id }) => {
      await queryClient.cancelQueries({ queryKey: lists });
      const before = queryClient.getQueriesData<IssueSummary[]>({ queryKey: lists });
      const state = states.find((s) => s.id === state_id);
      if (state) {
        queryClient.setQueriesData<IssueSummary[]>({ queryKey: lists }, (list) =>
          list?.map((i) => (i.id === id ? { ...i, state } : i)),
        );
      }
      return { before };
    },
    onError: (_error, _move, context) => {
      for (const [key, list] of context?.before ?? []) queryClient.setQueryData(key, list);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: qk.issues.all }),
  });
  const columns = states.filter(
    (s) => !hideClosed || (s.category !== 'completed' && s.category !== 'canceled'),
  );
  // Every column takes drops, folded or not, with or without cards.
  const dropTarget = (state: IssueState) => ({
    'data-over': over === state.id,
    onDragOver: (e: DragEvent) => {
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
      setOver(state.id);
    },
    onDragLeave: () => setOver((current) => (current === state.id ? null : current)),
    onDrop: (e: DragEvent) => {
      e.preventDefault();
      setOver(null);
      const id = e.dataTransfer.getData('text/plain');
      const issue = issues.find((i) => i.id === id);
      if (issue && issue.state.id !== state.id) move.mutate({ id, state_id: state.id });
    },
  });

  return (
    <ScrollRegion
      label="Board"
      // Columns come and go, and fold, inside a box whose own size stays the same.
      measureKey={`${columns.length}:${folded.join()}`}
      className="flex min-h-64 flex-1 gap-2 overflow-x-auto pb-1"
    >
      {columns.map((state) => {
        const cards = issues.filter((i) => i.state.id === state.id);
        if (folded.includes(state.id)) {
          return (
            <section
              key={state.id}
              aria-label={state.name}
              className="flex w-10 shrink-0 rounded-xl border bg-muted/30 transition-colors data-[over=true]:border-ring data-[over=true]:bg-muted/60"
              {...dropTarget(state)}
            >
              <button
                ref={focusIfToggled(state.id)}
                type="button"
                aria-label={`Unfold ${state.name} (${cards.length})`}
                onClick={(e) => toggle(state.id, e)}
                className="flex w-full flex-col items-center gap-2 rounded-xl py-3 text-[13px] font-medium outline-none hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                <StateGlyph category={state.category} color={state.color} />
                <span className="text-xs font-normal text-muted-foreground tabular-nums">
                  {cards.length}
                </span>
                <span className="whitespace-nowrap [writing-mode:vertical-rl]">{state.name}</span>
              </button>
            </section>
          );
        }
        return (
          <section
            key={state.id}
            aria-label={state.name}
            className="flex w-72 shrink-0 flex-col rounded-xl border bg-muted/30 transition-colors data-[over=true]:border-ring data-[over=true]:bg-muted/60"
            {...dropTarget(state)}
          >
            <div className="flex h-9 shrink-0 items-center gap-1 pr-1.5 pl-3">
              <h2 className="flex min-w-0 flex-1 items-center gap-2 text-[13px] font-medium">
                <StateGlyph category={state.category} color={state.color} />
                <span className="truncate">{state.name}</span>
                <span className="text-xs font-normal text-muted-foreground tabular-nums">
                  {cards.length}
                </span>
              </h2>
              <IconButton
                ref={focusIfToggled(state.id)}
                label={`Fold ${state.name}`}
                className="size-6 text-muted-foreground"
                onClick={(e) => toggle(state.id, e)}
              >
                <FoldHorizontalIcon />
              </IconButton>
              <IconButton
                label={`New issue in ${state.name}`}
                className="size-6 text-muted-foreground"
                onClick={() => onCreate(state.id)}
              >
                <PlusIcon />
              </IconButton>
            </div>
            <ul className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto px-1.5 pb-1.5">
              {cards.length === 0 && (
                <li className="px-1.5 py-1 text-xs text-muted-foreground">No issues</li>
              )}
              {cards.map((issue) => (
                <li key={issue.id}>
                  <BoardCard issue={issue} />
                </li>
              ))}
            </ul>
          </section>
        );
      })}
    </ScrollRegion>
  );
}

/**
 * One issue on the board. The whole card opens the issue (its title is the link, stretched over
 * the card, and its tooltip names what the glyphs under it show) and drags it; the GitHub chip
 * opens GitHub instead.
 */
function BoardCard({ issue }: { issue: IssueSummary }) {
  const queryClient = useQueryClient();
  return (
    <div
      {...onIntent(() => prefetchIssue(queryClient, issue))}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData('text/plain', issue.id);
        e.dataTransfer.effectAllowed = 'move';
      }}
      className="relative cursor-grab space-y-1 rounded-lg border bg-background px-2.5 py-2 shadow-xs transition-colors hover:border-ring/60 active:cursor-grabbing has-[a[data-card-link]:focus-visible]:border-ring motion-safe:animate-fade-in"
    >
      <span className="flex min-w-0 items-center gap-1.5 font-mono text-xs text-muted-foreground">
        <span className="shrink-0">{issue.identifier}</span>
        {issue.github_url && <GithubChip url={issue.github_url} form="repo" className="min-w-0" />}
        {issue.graph_id && <NetworkIcon className="size-3 shrink-0" aria-label="Has a graph" />}
        <PersonGlyph name={issue.assignee?.name} className="ml-auto font-sans" />
      </span>
      <Link
        to={issuePath(issue.id)}
        draggable={false}
        data-card-link
        title={issueLinkTitle(issue)}
        className="line-clamp-2 text-[13px] outline-none after:absolute after:inset-0 after:rounded-lg"
      >
        <span className="sr-only">{issue.identifier} </span>
        {issue.title}
      </Link>
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
    </div>
  );
}
