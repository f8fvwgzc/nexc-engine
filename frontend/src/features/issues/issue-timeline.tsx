import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowDownUpIcon, ChevronRightIcon, ListFilterIcon } from 'lucide-react';
import { useId, useMemo, useState } from 'react';

import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Skeleton } from '@/components/ui/skeleton';
import { artifactFilesQuery } from '@/features/artifacts/api';
import { errorMessage } from '@/lib/api/errors';
import { MotionPresence } from '@/lib/motion';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import type { Issue, IssueEvent } from '@/schemas/issue';
import { useAuthStore } from '@/stores/auth-store';

import {
  ACTIVITY_SHOW_OPTIONS,
  groupActivity,
  showsEvent,
  threadReplies,
  useActivityPrefs,
} from './activity';
import { ArtifactEntry, ChangeEntry, EntryAvatar, EntryGrid, EntryRow } from './activity-entry';
import { createComment, issueEventsQuery } from './api';
import { CommentEntry, type CommentContext, type DraftUpdate } from './comment-entry';
import { Composer } from './composer';
import { RunReport } from './issue-run';
import { EMPTY_DRAFT, mentionsIn, toWire, type MentionDraft, type MentionKind } from './mentions';
import { AddReaction, ReactionBar, ReactionSummary } from './reaction-bar';
import { useMentionables } from './use-mentionables';
import { useReactions } from './use-reactions';

/** The line under the agents in the mention list: what mentioning one will do. */
function agentNote(canPlan: boolean | null): string | undefined {
  if (canPlan === null) return undefined;
  return canPlan
    ? 'An active agent you mention takes the issue if it has no agent yet, and is asked for a plan.'
    : 'An active agent you mention takes the issue if it has no agent yet. Join the team to have it plan.';
}

/** Whether the text a comment was sent with mentions an agent: the issue may have changed. */
const mentionsAgent = (body: string) => mentionsIn(body).some((m) => m.kind === 'agent');

/** A change to one of the timeline's drafts, by comment id. */
const changeDraft =
  (id: string, next: DraftUpdate) => (all: Record<string, MentionDraft | null>) => ({
    ...all,
    [id]: typeof next === 'function' ? next(all[id] ?? null) : next,
  });

/**
 * The comments on an issue and their threads, the history of its state, priority, assignee,
 * title, GitHub link and files, what its runs reported and what mentions handed its agents, with
 * a box to add a comment (`@` mentions a teammate or an agent). Each entry has its kind's mark,
 * its author's avatar and the time on the right; changes one person made in a row read as one
 * entry, runs and files sit on cards, and comments and run reports take reactions. The header
 * folds it away and picks what it shows and in which order; the choice is the member's, the same
 * on every issue.
 */
export function IssueTimeline({ issue, canModerate }: { issue: Issue; canModerate: boolean }) {
  const queryClient = useQueryClient();
  const user = useAuthStore((s) => s.user);
  const bodyId = useId();
  const [draft, setDraft] = useState<MentionDraft>(EMPTY_DRAFT);
  // Comment edits and replies in progress, by comment id (null once sent or given up).
  const [edits, setEdits] = useState<Record<string, MentionDraft | null>>({});
  const [replies, setReplies] = useState<Record<string, MentionDraft | null>>({});
  const [prefs, setPrefs] = useActivityPrefs();
  const { data: events, isPending, error } = useQuery(issueEventsQuery(issue.id));
  const react = useReactions(issue.id);
  // A file entry offers the file while the issue still has one of that name.
  const { data: files } = useQuery({
    ...artifactFilesQuery(issue.workspace_id, { issue_id: issue.id }),
    enabled: (events ?? []).some((e) => e.kind === 'artifact'),
  });
  // Whom to mention is read once someone starts to write; before that, only what the comments'
  // mentions need to show.
  const [writing, setWriting] = useState(false);
  const mentioned = useMemo(() => {
    const kinds = new Set<MentionKind>();
    for (const e of events ?? [])
      if (e.kind === 'comment') for (const m of mentionsIn(e.body)) kinds.add(m.kind);
    return kinds;
  }, [events]);
  const mentionables = useMentionables(issue, {
    write: writing,
    users: mentioned.has('user'),
    agents: mentioned.has('agent'),
  });

  /** After a comment was added, edited or deleted: one that was added shows at once. */
  const changed = (body: string, posted?: IssueEvent) => {
    if (posted) {
      queryClient.setQueryData<IssueEvent[]>(qk.issues.events(issue.id), (all) =>
        all && !all.some((e) => e.id === posted.id) ? [...all, posted] : all,
      );
      // Posted while the filter hides comments, it would seem to vanish: show everything instead.
      if (prefs.show !== 'all' && prefs.show !== 'comments') setPrefs({ show: 'all' });
    }
    void queryClient.invalidateQueries({ queryKey: qk.issues.events(issue.id) });
    // A mentioned agent may have been delegated the issue and given it a graph to plan in.
    if (mentionsAgent(body))
      void queryClient.invalidateQueries({ queryKey: qk.issues.one(issue.id) });
  };

  // A change nobody made in the same moment a run reported is the run's: it moved the issue.
  const reported = new Set((events ?? []).filter((e) => e.kind === 'run').map((e) => e.created_at));
  const byAgents = (event: IssueEvent) => event.actor === null && reported.has(event.created_at);
  const nameOf = (event: IssueEvent) =>
    event.actor?.name ?? (byAgents(event) ? 'Agents' : 'Someone who left');
  const shown = (events ?? []).filter((event) => showsEvent(event, prefs.show));
  // The server sends the oldest first; a thread reads oldest first whatever the order.
  const thread = threadReplies(shown);
  const ordered = prefs.order === 'desc' ? [...thread.entries].reverse() : thread.entries;
  const items = groupActivity(
    ordered,
    (event) => event.actor?.user_id ?? (byAgents(event) ? 'agents' : 'left'),
  );
  const showLabel = ACTIVITY_SHOW_OPTIONS.find((o) => o.value === prefs.show)?.label;

  const comments: CommentContext = {
    issueId: issue.id,
    me: user?.id,
    myName: user?.name,
    canModerate,
    mentionables,
    agentNote: agentNote(mentionables.canPlan),
    startWriting: () => setWriting(true),
    editDraft: (id) => edits[id] ?? null,
    setEditDraft: (id, next) => setEdits(changeDraft(id, next)),
    replyDraft: (id) => replies[id] ?? null,
    setReplyDraft: (id, next) => setReplies(changeDraft(id, next)),
    onChanged: changed,
    react,
  };

  const add = useMutation({
    mutationFn: (body: string) => createComment(issue.id, body),
    meta: { errorToast: false },
    onSuccess: (posted, body) => {
      setDraft(EMPTY_DRAFT);
      changed(body, posted);
    },
  });
  const send = () => {
    if (draft.text.trim() && !add.isPending) add.mutate(toWire(draft.text.trim(), draft.picked));
  };

  const composer = (
    <EntryGrid>
      <EntryRow avatar={<EntryAvatar name={user?.name} />}>
        <Composer
          draft={draft}
          onDraftChange={setDraft}
          onSend={send}
          sending={add.isPending}
          label="New comment"
          placeholder="Leave a comment…"
          candidates={mentionables.candidates}
          agentNote={comments.agentNote}
          onFocus={() => setWriting(true)}
        />
        {add.error && (
          <p role="alert" className="mt-1 text-sm text-destructive">
            {errorMessage(add.error)}
          </p>
        )}
      </EntryRow>
    </EntryGrid>
  );

  return (
    <section aria-label="Activity" className="space-y-3">
      <div className="flex items-center gap-1">
        <h2 className="min-w-0 flex-1">
          <button
            type="button"
            aria-expanded={!prefs.collapsed}
            aria-controls={bodyId}
            onClick={() => setPrefs({ collapsed: !prefs.collapsed })}
            className="flex items-center gap-1.5 rounded-sm text-[13px] font-medium text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            <ChevronRightIcon
              aria-hidden
              className={cn('size-3.5 transition-transform', !prefs.collapsed && 'rotate-90')}
            />
            Activity
            {events && <span className="text-xs font-normal tabular-nums">{shown.length}</span>}
          </button>
        </h2>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="sm"
              aria-label={`Show: ${showLabel}`}
              className="h-6 gap-1.5 px-2 text-xs text-muted-foreground"
            >
              <ListFilterIcon className="size-3.5" />
              {showLabel}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-40">
            <DropdownMenuLabel>Show</DropdownMenuLabel>
            <DropdownMenuRadioGroup
              value={prefs.show}
              onValueChange={(value) => {
                const match = ACTIVITY_SHOW_OPTIONS.find((o) => o.value === value);
                if (match) setPrefs({ show: match.value });
              }}
            >
              {ACTIVITY_SHOW_OPTIONS.map((option) => (
                <DropdownMenuRadioItem key={option.value} value={option.value}>
                  {option.label}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
        <Button
          variant="ghost"
          size="sm"
          title={prefs.order === 'asc' ? 'Show the newest first' : 'Show the oldest first'}
          className="h-6 gap-1.5 px-2 text-xs text-muted-foreground"
          onClick={() => setPrefs({ order: prefs.order === 'asc' ? 'desc' : 'asc' })}
        >
          <ArrowDownUpIcon className="size-3.5" />
          {prefs.order === 'asc' ? 'Oldest first' : 'Newest first'}
        </Button>
      </div>
      {prefs.collapsed ? (
        events && (
          <p id={bodyId} className="text-[13px] text-muted-foreground">
            {shown.length} {shown.length === 1 ? 'entry' : 'entries'} hidden
          </p>
        )
      ) : (
        <div id={bodyId} className="space-y-5">
          {prefs.order === 'desc' && composer}
          {isPending ? (
            <div className="space-y-2">
              <Skeleton className="h-4 w-2/3" />
              <Skeleton className="h-14 w-full" />
            </div>
          ) : error ? (
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(error)}
            </p>
          ) : (
            // One-line changes in a row sit closer together than the entries around them.
            <ol className="space-y-4 [&>[data-compact]:has(+[data-compact])]:mb-2">
              {items.length === 0 && (
                <li className="text-[13px] text-muted-foreground">
                  {events.length === 0
                    ? 'Nothing yet. Changes to this issue and comments show up here.'
                    : 'Nothing of this kind yet. Pick All activity to see every entry.'}
                </li>
              )}
              <MotionPresence>
                {items.map((item) => {
                  if (item.kind === 'changes') {
                    const first = item.events[0];
                    if (!first) return null;
                    return (
                      <ChangeEntry
                        key={first.id}
                        events={item.events}
                        name={nameOf(first)}
                        person={first.actor?.name ?? (byAgents(first) ? 'Agents' : undefined)}
                      />
                    );
                  }
                  const { event } = item;
                  const toggle = (emoji: string, on: boolean) => react(event.id, emoji, on);
                  if (event.kind === 'run') {
                    return (
                      <RunReport
                        key={event.id}
                        event={event}
                        actions={
                          // Once it has reactions, their bubble holds the `+`.
                          event.reactions.length === 0 && (
                            <AddReaction
                              reactions={event.reactions}
                              onToggle={toggle}
                              align="end"
                              className="size-7"
                            />
                          )
                        }
                        footer={
                          <>
                            <ReactionBar
                              reactions={event.reactions}
                              me={user?.id}
                              onToggle={toggle}
                            />
                            {event.reactions.length > 0 && (
                              <div className="mt-2">
                                <ReactionSummary reactions={event.reactions} me={user?.id} />
                              </div>
                            )}
                          </>
                        }
                      />
                    );
                  }
                  if (event.kind === 'artifact') {
                    return (
                      <ArtifactEntry
                        key={event.id}
                        event={event}
                        name={nameOf(event)}
                        file={files?.find((f) => f.name === event.body)}
                      />
                    );
                  }
                  return (
                    <CommentEntry
                      key={event.id}
                      event={event}
                      replies={thread.replies.get(event.id) ?? []}
                      comments={comments}
                    />
                  );
                })}
              </MotionPresence>
            </ol>
          )}
          {prefs.order === 'asc' && composer}
        </div>
      )}
    </section>
  );
}
