import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { Textarea } from '@/components/ui/textarea';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import type { Issue, IssueEvent } from '@/schemas/issue';
import { useAuthStore } from '@/stores/auth-store';

import { createComment, deleteComment, issueEventsQuery, updateComment } from './api';

/** What a change entry says after the actor's name. */
function changeText(event: IssueEvent): string {
  const { from, to } = event;
  switch (event.kind) {
    case 'state':
      return `moved this from ${from ?? 'no state'} to ${to ?? 'no state'}`;
    case 'priority':
      return `set priority to ${to ?? 'none'}`;
    case 'assignee':
      if (!to) return `unassigned ${from ?? 'the assignee'}`;
      return `assigned this to ${to}`;
    case 'title':
      return `renamed this to “${to ?? ''}”`;
    case 'due':
      if (!to) return 'removed the due date';
      return from ? `moved the due date from ${from} to ${to}` : `set the due date to ${to}`;
    case 'comment':
      return 'commented';
  }
}

function Comment({
  event,
  issueId,
  own,
  canDelete,
  onChanged,
}: {
  event: IssueEvent;
  issueId: string;
  own: boolean;
  canDelete: boolean;
  onChanged: () => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const edit = useMutation({
    mutationFn: (body: string) => updateComment(issueId, event.id, body),
    onSuccess: () => {
      setDraft(null);
      onChanged();
    },
  });
  const remove = useMutation({
    mutationFn: () => deleteComment(issueId, event.id),
    onSuccess: onChanged,
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
              onClick={() => setDraft(event.body)}
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
            autoFocus
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
 * The comments on an issue and the history of its state, priority, assignee and title, oldest
 * first, with a box to add a comment.
 */
export function IssueTimeline({ issue, canModerate }: { issue: Issue; canModerate: boolean }) {
  const queryClient = useQueryClient();
  const me = useAuthStore((s) => s.user?.id);
  const [body, setBody] = useState('');
  const { data: events, isPending, error } = useQuery(issueEventsQuery(issue.id));
  const reload = () => void queryClient.invalidateQueries({ queryKey: qk.issues.events(issue.id) });
  const add = useMutation({
    mutationFn: () => createComment(issue.id, body.trim()),
    meta: { errorToast: false },
    onSuccess: () => {
      setBody('');
      reload();
    },
  });

  return (
    <section aria-label="Activity" className="space-y-3 border-t pt-4">
      <h3 className="text-[13px] font-medium text-muted-foreground">Activity</h3>
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
          {events.length === 0 && (
            <li className="text-[13px] text-muted-foreground">
              Nothing yet. Changes to this issue and comments show up here.
            </li>
          )}
          {events.map((event) =>
            event.kind === 'comment' ? (
              <Comment
                key={event.id}
                event={event}
                issueId={issue.id}
                own={event.actor?.user_id === me}
                canDelete={event.actor?.user_id === me || canModerate}
                onChanged={reload}
              />
            ) : (
              <li key={event.id} className="flex items-center gap-2 px-3 text-[13px]">
                <PersonGlyph name={event.actor?.name} />
                <span className="min-w-0 truncate text-muted-foreground">
                  <span className="font-medium text-foreground">
                    {event.actor?.name ?? 'Someone who left'}
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
    </section>
  );
}
