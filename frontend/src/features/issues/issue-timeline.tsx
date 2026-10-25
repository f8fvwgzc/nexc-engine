import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowDownUpIcon, ChevronRightIcon, ListFilterIcon } from 'lucide-react';
import { useId, useState } from 'react';

import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Message, MessageAvatar, MessageContent } from '@/components/ui/message';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import type { Issue, IssueEvent } from '@/schemas/issue';
import { useAuthStore } from '@/stores/auth-store';

import { ACTIVITY_SHOW_OPTIONS, changeText, showsEvent, useActivityPrefs } from './activity';
import { createComment, deleteComment, issueEventsQuery, updateComment } from './api';
import { RunReport } from './issue-run';
import { MentionText, MentionTextarea } from './mention-textarea';
import { addPicked, fromWire, mentionsIn, toWire, type Mentionable } from './mentions';
import { useMentionables, type Mentionables } from './use-mentionables';

/** A comment as it is being written: the text shown, and the mentions picked into it. */
interface Draft {
  text: string;
  picked: Mentionable[];
}

const EMPTY_DRAFT: Draft = { text: '', picked: [] };

/** A new draft, or a change to the one there is. */
type DraftUpdate = Draft | null | ((current: Draft | null) => Draft | null);

/** The comment field: one soft surface, outlined only while it is being typed in. */
const COMPOSER =
  'rounded-lg border border-transparent bg-muted/40 transition-colors focus-within:border-input focus-within:bg-background dark:focus-within:bg-input/30';
const COMPOSER_FIELD =
  'min-h-10 resize-none border-0 bg-transparent px-3 py-2.5 shadow-none focus-visible:ring-0 dark:bg-transparent';

/** The line under the agents in the mention list: what mentioning one will do. */
function agentNote(canPlan: boolean | null): string | undefined {
  if (canPlan === null) return undefined;
  return canPlan
    ? 'A mentioned agent is delegated the issue and asked for a plan.'
    : 'A mentioned agent is delegated the issue. Join the team to have it plan.';
}

/** Whether the text a comment was sent with mentions an agent: the issue may have changed. */
const mentionsAgent = (body: string) => mentionsIn(body).some((m) => m.kind === 'agent');

/**
 * One comment, with Edit for its author and Delete for its author and moderators: the author's
 * avatar beside the text, no card around it. The edit in progress is the timeline's (`draft`, null
 * while not editing), so it outlasts this comment being folded away or filtered out.
 */
function Comment({
  event,
  issueId,
  own,
  canDelete,
  draft,
  setDraft,
  onChanged,
  mentionables,
  onEditing,
}: {
  event: IssueEvent;
  issueId: string;
  own: boolean;
  canDelete: boolean;
  draft: Draft | null;
  setDraft: (next: DraftUpdate) => void;
  onChanged: (body: string) => void;
  mentionables: Mentionables;
  /** Called when an edit starts, so the mention list has its people. */
  onEditing: () => void;
}) {
  // The field takes focus when Edit is clicked, not when an edit in progress comes back into view.
  const [focusEdit, setFocusEdit] = useState(false);
  const edit = useMutation({
    mutationFn: (body: string) => updateComment(issueId, event.id, body),
    onSuccess: (_, body) => {
      setDraft(null);
      onChanged(body);
    },
  });
  const remove = useMutation({
    mutationFn: () => deleteComment(issueId, event.id),
    onSuccess: () => {
      setDraft(null);
      onChanged('');
    },
  });
  const save = () => {
    if (draft?.text.trim()) edit.mutate(toWire(draft.text.trim(), draft.picked));
  };
  const name = event.actor?.name ?? 'Someone who left';
  return (
    <li>
      <Message className="gap-2.5">
        <MessageAvatar className="mt-px min-w-0 self-start bg-transparent">
          <PersonGlyph name={event.actor?.name} className="size-6 text-[10px]" />
        </MessageAvatar>
        <MessageContent className="gap-1">
          <div className="flex min-h-6 items-center gap-2 text-[13px]">
            <span className="truncate font-medium">{name}</span>
            <span className="shrink-0 text-muted-foreground">
              {formatRelative(event.created_at)}
              {event.edited_at ? ' · edited' : ''}
            </span>
            <span className="ml-auto flex shrink-0 gap-1">
              {own && draft === null && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 px-2 text-xs"
                  onClick={() => {
                    setFocusEdit(true);
                    onEditing();
                    setDraft(fromWire(event.body));
                  }}
                >
                  Edit
                </Button>
              )}
              {canDelete && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 px-2 text-xs text-destructive"
                  disabled={remove.isPending}
                  aria-label={`Delete comment by ${name}`}
                  onClick={() => remove.mutate()}
                >
                  Delete
                </Button>
              )}
            </span>
          </div>
          {draft === null ? (
            <MentionText body={event.body} mentionables={mentionables} />
          ) : (
            <form
              className={COMPOSER}
              onSubmit={(e) => {
                e.preventDefault();
                save();
              }}
            >
              <MentionTextarea
                autoFocus={focusEdit}
                rows={3}
                value={draft.text}
                aria-label="Edit comment"
                className={COMPOSER_FIELD}
                candidates={mentionables.candidates}
                agentNote={agentNote(mentionables.canPlan)}
                onFocus={(e) =>
                  e.target.setSelectionRange(e.target.value.length, e.target.value.length)
                }
                onValueChange={(text) => setDraft((d) => d && { ...d, text })}
                onPick={(m) => setDraft((d) => d && { ...d, picked: addPicked(d.picked, m) })}
                onSubmit={save}
              />
              <div className="flex justify-end gap-2 px-2 pb-2">
                <Button type="button" variant="ghost" size="sm" onClick={() => setDraft(null)}>
                  Cancel
                </Button>
                <Button type="submit" size="sm" disabled={!draft.text.trim() || edit.isPending}>
                  Save
                </Button>
              </div>
            </form>
          )}
          {(edit.error ?? remove.error) && (
            <p role="alert" className="text-xs text-destructive">
              {errorMessage(edit.error ?? remove.error)}
            </p>
          )}
        </MessageContent>
      </Message>
    </li>
  );
}

/**
 * The comments on an issue, the history of its state, priority, assignee, title, GitHub link and
 * files, what its runs reported and what mentions handed its agents, with a box to add a comment
 * (`@` mentions a teammate or an agent). The header folds it away and picks what it shows and in
 * which order; the choice is the member's, the same on every issue.
 */
export function IssueTimeline({ issue, canModerate }: { issue: Issue; canModerate: boolean }) {
  const queryClient = useQueryClient();
  const me = useAuthStore((s) => s.user?.id);
  const bodyId = useId();
  const [draft, setDraftState] = useState<Draft>(EMPTY_DRAFT);
  // Comment edits in progress, by comment id (null once saved or given up).
  const [drafts, setDrafts] = useState<Record<string, Draft | null>>({});
  const setDraft = (eventId: string) => (next: DraftUpdate) =>
    setDrafts((all) => ({
      ...all,
      [eventId]: typeof next === 'function' ? next(all[eventId] ?? null) : next,
    }));
  // The people and agents to mention are read once someone starts to write.
  const [writing, setWriting] = useState(false);
  const mentionables = useMentionables(issue, writing);
  const [prefs, setPrefs] = useActivityPrefs();
  const { data: events, isPending, error } = useQuery(issueEventsQuery(issue.id));
  const reload = (body = '') => {
    void queryClient.invalidateQueries({ queryKey: qk.issues.events(issue.id) });
    // A mentioned agent may have been delegated the issue and given it a graph to plan in.
    if (mentionsAgent(body))
      void queryClient.invalidateQueries({ queryKey: qk.issues.one(issue.id) });
  };
  // A change nobody made in the same moment a run reported is the run's: it moved the issue.
  const reported = new Set((events ?? []).filter((e) => e.kind === 'run').map((e) => e.created_at));
  const byAgents = (event: IssueEvent) => event.actor === null && reported.has(event.created_at);
  const shown = (events ?? []).filter((event) => showsEvent(event, prefs.show));
  // The server sends the oldest first.
  const ordered = prefs.order === 'desc' ? [...shown].reverse() : shown;
  const showLabel = ACTIVITY_SHOW_OPTIONS.find((o) => o.value === prefs.show)?.label;
  const add = useMutation({
    mutationFn: (body: string) => createComment(issue.id, body),
    meta: { errorToast: false },
    onSuccess: (_, body) => {
      setDraftState(EMPTY_DRAFT);
      // Posted while the filter hides comments, it would seem to vanish: show everything instead.
      if (prefs.show !== 'all' && prefs.show !== 'comments') setPrefs({ show: 'all' });
      reload(body);
    },
  });
  const send = () => {
    if (draft.text.trim() && !add.isPending) add.mutate(toWire(draft.text.trim(), draft.picked));
  };

  const composer = (
    <form
      className="space-y-2"
      onSubmit={(e) => {
        e.preventDefault();
        send();
      }}
    >
      <div className={COMPOSER}>
        <MentionTextarea
          rows={2}
          value={draft.text}
          placeholder="Leave a comment… Type @ to mention a teammate or an agent"
          aria-label="New comment"
          className={COMPOSER_FIELD}
          candidates={mentionables.candidates}
          agentNote={agentNote(mentionables.canPlan)}
          onFocus={() => setWriting(true)}
          onValueChange={(text) => setDraftState((d) => ({ ...d, text }))}
          onPick={(m) => setDraftState((d) => ({ ...d, picked: addPicked(d.picked, m) }))}
          onSubmit={send}
        />
        <div className="flex justify-end px-2 pb-2">
          <Button type="submit" size="sm" disabled={!draft.text.trim() || add.isPending}>
            Comment
          </Button>
        </div>
      </div>
      {add.error && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(add.error)}
        </p>
      )}
    </form>
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
        <div id={bodyId} className="space-y-3">
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
            <ol className="space-y-3">
              {ordered.length === 0 && (
                <li className="text-[13px] text-muted-foreground">
                  {events.length === 0
                    ? 'Nothing yet. Changes to this issue and comments show up here.'
                    : 'Nothing of this kind yet. Pick All activity to see every entry.'}
                </li>
              )}
              {ordered.map((event) =>
                event.kind === 'run' ? (
                  <RunReport key={event.id} event={event} />
                ) : event.kind === 'comment' ? (
                  <Comment
                    key={event.id}
                    event={event}
                    issueId={issue.id}
                    own={event.actor?.user_id === me}
                    canDelete={event.actor?.user_id === me || canModerate}
                    draft={drafts[event.id] ?? null}
                    setDraft={setDraft(event.id)}
                    onChanged={reload}
                    mentionables={mentionables}
                    onEditing={() => setWriting(true)}
                  />
                ) : (
                  <li key={event.id} className="flex items-center gap-2.5 text-[13px]">
                    <span className="flex w-6 shrink-0 justify-center">
                      <PersonGlyph
                        name={event.actor?.name ?? (byAgents(event) ? 'Agents' : undefined)}
                      />
                    </span>
                    <span className="min-w-0 truncate text-muted-foreground">
                      <span className="font-medium text-foreground">
                        {event.actor?.name ?? (byAgents(event) ? 'Agents' : 'Someone who left')}
                      </span>{' '}
                      {changeText(event)}
                    </span>
                    <span className="ml-auto shrink-0 text-xs text-muted-foreground">
                      {formatRelative(event.created_at)}
                    </span>
                  </li>
                ),
              )}
            </ol>
          )}
          {prefs.order === 'asc' && composer}
        </div>
      )}
    </section>
  );
}
