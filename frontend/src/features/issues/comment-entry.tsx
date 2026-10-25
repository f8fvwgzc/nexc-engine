import { useMutation } from '@tanstack/react-query';
import { PencilIcon, ReplyIcon, Trash2Icon } from 'lucide-react';
import { useRef, useState, type ComponentProps, type Ref } from 'react';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { IconButton } from '@/components/custom-ui/icon-button';
import { Button } from '@/components/ui/button';
import { DropdownMenuItem } from '@/components/ui/dropdown-menu';
import { errorMessage } from '@/lib/api/errors';
import { MotionPresence, rise } from '@/lib/motion';
import { cn } from '@/lib/utils';
import type { IssueEvent } from '@/schemas/issue';

import {
  ActivityEntry,
  EntryAvatar,
  EntryHeader,
  EntryMenu,
  EntryRow,
  KindIcon,
  ReactionsIcon,
} from './activity-entry';
import { createComment, deleteComment, updateComment } from './api';
import { Composer } from './composer';
import { MentionText, MentionTextarea } from './mention-textarea';
import {
  addPicked,
  EMPTY_DRAFT,
  fromWire,
  pickedLabel,
  toWire,
  type MentionDraft,
} from './mentions';
import { AddReaction, ReactionBar, ReactionSummary } from './reaction-bar';
import type { Mentionables } from './use-mentionables';

/** A new draft, or a change to the one there is. */
export type DraftUpdate =
  MentionDraft | null | ((current: MentionDraft | null) => MentionDraft | null);

/**
 * What a comment needs from the timeline it is in. The drafts are the timeline's, so an edit or a
 * reply being written outlasts its comment being folded away or filtered out.
 */
export interface CommentContext {
  issueId: string;
  me: string | undefined;
  /** The reader's name, for their avatar beside a reply box. */
  myName: string | undefined;
  canModerate: boolean;
  mentionables: Mentionables;
  /** The line under the agents in the mention list. */
  agentNote: string | undefined;
  /** Someone started to write: the mention list reads whom it offers. */
  startWriting: () => void;
  /** The edit in progress of a comment; null while it is not being edited. */
  editDraft: (commentId: string) => MentionDraft | null;
  setEditDraft: (commentId: string, next: DraftUpdate) => void;
  /** The reply being written under a comment; null while its box is closed. */
  replyDraft: (commentId: string) => MentionDraft | null;
  setReplyDraft: (commentId: string, next: DraftUpdate) => void;
  /** A comment was added (the one the server answered with), edited or deleted. */
  onChanged: (body: string, posted?: IssueEvent) => void;
  react: (eventId: string, emoji: string, on: boolean) => void;
}

/** The soft field an edit is made in, outlined with the focus ring only while it is typed in. */
const EDITOR =
  'rounded-2xl border border-transparent bg-muted/60 transition-[color,background-color,border-color,box-shadow] focus-within:border-ring focus-within:bg-background focus-within:ring-3 focus-within:ring-ring/50 dark:bg-muted/50 dark:focus-within:bg-input/30';

/**
 * A comment, or a reply in a comment's thread: the author's avatar, "**Name** commented" and the
 * time, the text, and its reactions. On hover (always on a touch screen) it offers a reaction, a
 * reply, and in its menu Edit for its author and Delete for its author and moderators. Deleting a
 * comment deletes its thread too, everyone's replies in it, so that is asked first.
 */
function CommentRow({
  event,
  comments,
  reply,
  replies = 0,
  replyRef,
  threaded,
  onReply,
  ...motion
}: Omit<ComponentProps<typeof EntryRow>, 'children' | 'icon' | 'avatar'> & {
  event: IssueEvent;
  comments: CommentContext;
  /** A reply: a smaller avatar, no mark, no "commented". */
  reply: boolean;
  /** How many replies the comment has. */
  replies?: number;
  replyRef?: Ref<HTMLButtonElement>;
  onReply: () => void;
}) {
  const draft = comments.editDraft(event.id);
  const setDraft = (next: DraftUpdate) => comments.setEditDraft(event.id, next);
  // The field takes focus when Edit is picked, not when an edit in progress comes back into view.
  const [focusEdit, setFocusEdit] = useState(false);
  // Edit was picked: the menu, closing, leaves the focus to the field.
  const editPicked = useRef(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const edit = useMutation({
    mutationFn: (body: string) => updateComment(comments.issueId, event.id, body),
    onSuccess: (_, body) => {
      setDraft(null);
      comments.onChanged(body);
    },
  });
  const remove = useMutation({
    mutationFn: () => deleteComment(comments.issueId, event.id),
    onSuccess: () => {
      setDraft(null);
      comments.onChanged('');
    },
  });
  const save = () => {
    if (draft?.text.trim() && !edit.isPending) edit.mutate(toWire(draft.text.trim(), draft.picked));
  };
  const name = event.actor?.name ?? 'Someone who left';
  const own = event.actor !== null && event.actor.user_id === comments.me;
  const canDelete = own || comments.canModerate;
  const toggle = (emoji: string, on: boolean) => comments.react(event.id, emoji, on);

  // With a mouse, a small bar over the time while the comment is hovered or focused; on a touch
  // screen, always there beside it.
  const actions = (
    <span
      className={cn(
        'flex shrink-0 items-center gap-0.5 transition-opacity',
        'pointer-fine:pointer-events-none pointer-fine:absolute pointer-fine:top-0 pointer-fine:right-0 pointer-fine:rounded-full pointer-fine:bg-background pointer-fine:p-0.5 pointer-fine:opacity-0 pointer-fine:shadow-xs pointer-fine:ring-1 pointer-fine:ring-border/70',
        'group-focus-within/comment:pointer-events-auto group-focus-within/comment:opacity-100 group-hover/comment:pointer-events-auto group-hover/comment:opacity-100 has-[[aria-expanded=true]]:pointer-events-auto has-[[aria-expanded=true]]:opacity-100',
      )}
    >
      <AddReaction reactions={event.reactions} onToggle={toggle} align="end" />
      <IconButton
        ref={replyRef}
        label={reply ? `Reply to ${name}` : 'Reply'}
        size="icon-xs"
        className="rounded-full text-muted-foreground hover:text-foreground"
        onClick={onReply}
      >
        <ReplyIcon />
      </IconButton>
      {canDelete && (
        <EntryMenu
          label={`Actions for the comment by ${name}`}
          className="rounded-full"
          onCloseAutoFocus={(e) => {
            if (!editPicked.current) return;
            editPicked.current = false;
            e.preventDefault();
          }}
        >
          {own && (
            // Kept (not taken out) while the edit is open: the closing menu still holds it.
            <DropdownMenuItem
              disabled={draft !== null}
              onSelect={() => {
                editPicked.current = true;
                setFocusEdit(true);
                comments.startWriting();
                setDraft(fromWire(event.body));
              }}
            >
              <PencilIcon />
              Edit
            </DropdownMenuItem>
          )}
          <DropdownMenuItem
            variant="destructive"
            disabled={remove.isPending}
            onSelect={() => (replies > 0 ? setConfirmDelete(true) : remove.mutate())}
          >
            <Trash2Icon />
            Delete
          </DropdownMenuItem>
        </EntryMenu>
      )}
    </span>
  );

  return (
    <EntryRow
      icon={reply ? undefined : <KindIcon kind="comment" />}
      avatar={<EntryAvatar name={event.actor?.name} small={reply} />}
      threaded={threaded}
      className="group/comment"
      {...motion}
    >
      <EntryHeader
        name={name}
        action={reply ? undefined : 'commented'}
        at={event.created_at}
        edited={event.edited_at !== null}
        actions={actions}
      />
      {draft === null ? (
        <MentionText body={event.body} mentionables={comments.mentionables} className="mt-0.5" />
      ) : (
        <form
          className={`mt-1 ${EDITOR}`}
          onSubmit={(e) => {
            e.preventDefault();
            save();
          }}
        >
          <MentionTextarea
            autoFocus={focusEdit}
            rows={3}
            draft={draft}
            aria-label="Edit comment"
            className="min-h-10 resize-none border-0 bg-transparent px-3 py-2.5 shadow-none focus-visible:ring-0 dark:bg-transparent"
            candidates={comments.mentionables.candidates}
            agentNote={comments.agentNote}
            onFocus={(e) =>
              e.target.setSelectionRange(e.target.value.length, e.target.value.length)
            }
            // Not after the edit was saved or given up.
            onDraftChange={(next) => setDraft((d) => d && next)}
            onSubmit={save}
          />
          <div className="flex justify-end gap-2 px-2 pb-2">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="rounded-full"
              onClick={() => setDraft(null)}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              size="sm"
              className="rounded-full px-3.5"
              disabled={!draft.text.trim() || edit.isPending}
            >
              Save
            </Button>
          </div>
        </form>
      )}
      <ReactionBar reactions={event.reactions} me={comments.me} onToggle={toggle} />
      {(edit.error ?? remove.error) && (
        <p role="alert" className="mt-1 text-xs text-destructive">
          {errorMessage(edit.error ?? remove.error)}
        </p>
      )}
      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title={`Delete this comment and its ${replies === 1 ? 'reply' : `${replies} replies`}?`}
        description="Every reply in its thread goes with it, whoever wrote it, and the reactions to them. This cannot be undone."
        confirmLabel="Delete"
        destructive
        onConfirm={() => {
          setConfirmDelete(false);
          remove.mutate();
        }}
      />
    </EntryRow>
  );
}

/** A reply that starts by mentioning `person`, as answering a reply in the thread does. */
function mentioning(person: { user_id: string; name: string }): MentionDraft {
  const mention = { kind: 'user' as const, id: person.user_id, name: person.name };
  const picked = addPicked([], mention);
  return { text: `${pickedLabel(picked, mention)} `, picked };
}

/**
 * A comment as an entry of the activity, with its thread: who reacted to it, the replies under
 * it, oldest first, and a box to reply in. The box shows once the comment has replies, or when
 * Reply is clicked; Enter sends a reply, Escape closes an empty box nobody replied in yet.
 */
export function CommentEntry({
  event,
  replies,
  comments,
}: {
  event: IssueEvent;
  replies: IssueEvent[];
  comments: CommentContext;
}) {
  const draft = comments.replyDraft(event.id);
  const open = replies.length > 0 || draft !== null;
  // Clicking Reply puts the focus in the box: a new key mounts its field with the focus.
  const [focusReply, setFocusReply] = useState(0);
  // Where the focus goes back to when Escape closes the box.
  const replyButton = useRef<HTMLButtonElement>(null);
  const add = useMutation({
    mutationFn: (body: string) => createComment(comments.issueId, body, event.id),
    meta: { errorToast: false },
    onSuccess: (posted, body) => {
      // The box stays open under the thread.
      comments.setReplyDraft(event.id, EMPTY_DRAFT);
      comments.onChanged(body, posted);
    },
  });
  const send = () => {
    if (draft?.text.trim() && !add.isPending) add.mutate(toWire(draft.text.trim(), draft.picked));
  };
  const replyTo = (person?: IssueEvent['actor']) => {
    comments.startWriting();
    comments.setReplyDraft(event.id, (current) => {
      if (current?.text.trim()) return current;
      return person && person.user_id !== comments.me ? mentioning(person) : EMPTY_DRAFT;
    });
    setFocusReply((n) => n + 1);
  };
  return (
    <ActivityEntry>
      <CommentRow
        event={event}
        comments={comments}
        reply={false}
        replies={replies.length}
        replyRef={replyButton}
        threaded={open}
        onReply={() => replyTo()}
      />
      {event.reactions.length > 0 && (
        <EntryRow icon={<ReactionsIcon />} threaded={open}>
          <ReactionSummary reactions={event.reactions} me={comments.me} />
        </EntryRow>
      )}
      <MotionPresence>
        {replies.map((r, i) => (
          <CommentRow
            key={r.id}
            {...rise}
            event={r}
            comments={comments}
            reply
            threaded={i < replies.length - 1 || open}
            onReply={() => replyTo(r.actor)}
          />
        ))}
      </MotionPresence>
      {open && (
        <EntryRow avatar={<EntryAvatar name={comments.myName} small />}>
          <Composer
            key={focusReply}
            focusOnMount={focusReply > 0}
            draft={draft ?? EMPTY_DRAFT}
            onDraftChange={(next) => comments.setReplyDraft(event.id, next)}
            onSend={send}
            sending={add.isPending}
            label={`Reply to ${event.actor?.name ?? 'the comment'}`}
            sendLabel="Send reply"
            placeholder="Write a reply…"
            enterSends
            candidates={comments.mentionables.candidates}
            agentNote={comments.agentNote}
            onFocus={comments.startWriting}
            onKeyDown={(e) => {
              // An empty box nobody replied in yet goes away again, the focus back on Reply.
              if (e.key === 'Escape' && replies.length === 0 && !draft?.text) {
                e.preventDefault();
                replyButton.current?.focus();
                comments.setReplyDraft(event.id, null);
              }
            }}
          />
          {add.error && (
            <p role="alert" className="mt-1 text-xs text-destructive">
              {errorMessage(add.error)}
            </p>
          )}
        </EntryRow>
      )}
    </ActivityEntry>
  );
}
