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
import { Skeleton } from '@/components/ui/skeleton';
import { Textarea } from '@/components/ui/textarea';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import type { Issue, IssueEvent } from '@/schemas/issue';
import { useAuthStore } from '@/stores/auth-store';

import { ACTIVITY_SHOW_OPTIONS, changeText, showsEvent, useActivityPrefs } from './activity';
import { createComment, deleteComment, issueEventsQuery, updateComment } from './api';
import { RunReport } from './issue-run';

/**
 * One comment, with Edit for its author and Delete for its author and moderators. The edit in
 * progress is the timeline's (`draft`, null while not editing), so it outlasts this comment being
 * folded away or filtered out.
 */
function Comment({
  event,
  issueId,
  own,
  canDelete,
  draft,
  setDraft,
  onChanged,
}: {
  event: IssueEvent;
  issueId: string;
  own: boolean;
  canDelete: boolean;
  draft: string | null;
  setDraft: (draft: string | null) => void;
  onChanged: () => void;
}) {
  // The field takes focus when Edit is clicked, not when an edit in progress comes back into view.
  const [focusEdit, setFocusEdit] = useState(false);
  const edit = useMutation({
    mutationFn: (body: string) => updateComment(issueId, event.id, body),
    onSuccess: () => {
      setDraft(null);
      onChanged();
    },
  });
  const remove = useMutation({
    mutationFn: () => deleteComment(issueId, event.id),
    onSuccess: () => {
      setDraft(null);
      onChanged();
    },
  });
  const name = event.actor?.name ?? 'Someone who left';
  return (
    <li className="rounded-md border bg-card px-3 py-2">
      <div className="flex items-center gap-2 text-[13px]">
        <PersonGlyph name={event.actor?.name} />
        <span className="font-medium">{name}</span>
        <span className="text-muted-foreground">
          {formatRelative(event.created_at)}
          {event.edited_at ? ' · edited' : ''}
        </span>
        <span className="ml-auto flex gap-1">
          {own && draft === null && (
            <Button
              variant="ghost"
              size="sm"
              className="h-6 px-2 text-xs"
              onClick={() => {
                setFocusEdit(true);
                setDraft(event.body);
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
        <p className="mt-1.5 text-sm break-words whitespace-pre-wrap">{event.body}</p>
      ) : (
        <form
          className="mt-2 space-y-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (draft.trim()) edit.mutate(draft.trim());
          }}
        >
          <Textarea
            autoFocus={focusEdit}
            rows={3}
            value={draft}
            aria-label="Edit comment"
            onFocus={(e) =>
              e.target.setSelectionRange(e.target.value.length, e.target.value.length)
            }
            onChange={(e) => setDraft(e.target.value)}
          />
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" size="sm" onClick={() => setDraft(null)}>
              Cancel
            </Button>
            <Button type="submit" size="sm" disabled={!draft.trim() || edit.isPending}>
              Save
            </Button>
          </div>
        </form>
      )}
      {(edit.error ?? remove.error) && (
        <p role="alert" className="mt-1 text-xs text-destructive">
          {errorMessage(edit.error ?? remove.error)}
        </p>
      )}
    </li>
  );
}

/**
 * The comments on an issue, the history of its state, priority, assignee, title, GitHub link and
 * files, and what its runs reported, with a box to add a comment. The header folds it away and
 * picks what it shows and in which order; the choice is the member's, the same on every issue.
 */
export function IssueTimeline({ issue, canModerate }: { issue: Issue; canModerate: boolean }) {
  const queryClient = useQueryClient();
  const me = useAuthStore((s) => s.user?.id);
  const bodyId = useId();
  const [body, setBody] = useState('');
  // Comment edits in progress, by comment id (null once saved or given up).
  const [drafts, setDrafts] = useState<Record<string, string | null>>({});
  const setDraft = (eventId: string) => (draft: string | null) =>
    setDrafts((current) => ({ ...current, [eventId]: draft }));
  const [prefs, setPrefs] = useActivityPrefs();
  const { data: events, isPending, error } = useQuery(issueEventsQuery(issue.id));
  const reload = () => void queryClient.invalidateQueries({ queryKey: qk.issues.events(issue.id) });
  // A change nobody made in the same moment a run reported is the run's: it moved the issue.
  const reported = new Set((events ?? []).filter((e) => e.kind === 'run').map((e) => e.created_at));
  const byAgents = (event: IssueEvent) => event.actor === null && reported.has(event.created_at);
  const shown = (events ?? []).filter((event) => showsEvent(event, prefs.show));
  // The server sends the oldest first.
  const ordered = prefs.order === 'desc' ? [...shown].reverse() : shown;
  const showLabel = ACTIVITY_SHOW_OPTIONS.find((o) => o.value === prefs.show)?.label;
  const add = useMutation({
    mutationFn: () => createComment(issue.id, body.trim()),
    meta: { errorToast: false },
    onSuccess: () => {
      setBody('');
      // Posted while the filter hides comments, it would seem to vanish: show everything instead.
      if (prefs.show !== 'all' && prefs.show !== 'comments') setPrefs({ show: 'all' });
      reload();
    },
  });

  const composer = (
    <form
      className="space-y-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (body.trim()) add.mutate();
      }}
    >
      <Textarea
        rows={2}
        value={body}
        placeholder="Leave a comment…"
        aria-label="New comment"
        onChange={(e) => setBody(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && body.trim()) add.mutate();
        }}
      />
      {add.error && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(add.error)}
        </p>
      )}
      <div className="flex justify-end">
        <Button type="submit" size="sm" disabled={!body.trim() || add.isPending}>
          Comment
        </Button>
      </div>
    </form>
  );

  return (
    <section aria-label="Activity" className="space-y-3 border-t pt-4">
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
            <ol className="space-y-2">
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
                  />
                ) : (
                  <li key={event.id} className="flex items-center gap-2 px-3 text-[13px]">
                    <PersonGlyph
                      name={event.actor?.name ?? (byAgents(event) ? 'Agents' : undefined)}
                    />
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
