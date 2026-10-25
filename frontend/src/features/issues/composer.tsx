import { SmileIcon } from 'lucide-react';
import { useEffect, useEffectEvent, useId, useRef, type ComponentProps } from 'react';

import { IconButton } from '@/components/custom-ui/icon-button';
import { Button } from '@/components/ui/button';

import { MentionTextarea } from './mention-textarea';
import type { MentionDraft } from './mentions';
import { EmojiPicker } from './reaction-bar';
import type { MentionCandidate } from './use-mentionables';

type FieldProps = Pick<
  ComponentProps<typeof MentionTextarea>,
  'onFocus' | 'onKeyDown' | 'enterSends' | 'placeholder' | 'agentNote'
>;

/**
 * A box to write a comment or a reply in: one soft rounded field where `@` mentions a teammate or
 * an agent, a button that puts an emoji where the caret was, and Send. `label` names the field;
 * `sendLabel` the button, when there is more than one Send on the page. With `focusOnMount`, the
 * field takes the focus as it appears, the caret after what is already written.
 */
export function Composer({
  draft,
  onDraftChange,
  onSend,
  sending,
  label,
  sendLabel,
  candidates,
  focusOnMount = false,
  ...field
}: FieldProps & {
  draft: MentionDraft;
  onDraftChange: (draft: MentionDraft) => void;
  onSend: () => void;
  sending: boolean;
  label: string;
  sendLabel?: string;
  candidates: MentionCandidate[];
  focusOnMount?: boolean;
}) {
  const fieldId = useId();
  // Where the caret was when the field lost the focus (to the emoji button).
  const caret = useRef<number | null>(null);
  const focusAt = (at: number) => {
    const element = document.getElementById(fieldId);
    if (!(element instanceof HTMLTextAreaElement)) return;
    element.focus();
    element.setSelectionRange(at, at);
  };
  const focusEnd = useEffectEvent(() => focusAt(draft.text.length));
  useEffect(() => {
    if (focusOnMount) focusEnd();
  }, [focusOnMount]);
  const insert = (emoji: string) => {
    const at = Math.min(caret.current ?? draft.text.length, draft.text.length);
    onDraftChange({ ...draft, text: draft.text.slice(0, at) + emoji + draft.text.slice(at) });
    const after = at + emoji.length;
    caret.current = after;
    requestAnimationFrame(() => focusAt(after));
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        onSend();
      }}
      className="flex items-end gap-0.5 rounded-2xl bg-muted/60 py-1 pr-1 pl-3 transition-[background-color,box-shadow] focus-within:bg-muted focus-within:ring-2 focus-within:ring-ring/30 dark:bg-muted/50"
    >
      <MentionTextarea
        id={fieldId}
        rows={1}
        draft={draft}
        aria-label={label}
        className="max-h-60 min-h-8 flex-1 resize-none rounded-none border-0 bg-transparent px-0 py-1.5 shadow-none focus-visible:ring-0 dark:bg-transparent"
        candidates={candidates}
        onDraftChange={onDraftChange}
        onSubmit={onSend}
        onBlur={(e) => {
          caret.current = e.target.selectionStart;
        }}
        {...field}
      />
      <EmojiPicker
        label="Pick an emoji"
        align="end"
        onPick={insert}
        // The focus goes back into the field, after the emoji.
        onCloseAutoFocus={(e) => e.preventDefault()}
      >
        <IconButton
          label="Add an emoji"
          className="rounded-full text-muted-foreground hover:text-foreground"
        >
          <SmileIcon />
        </IconButton>
      </EmojiPicker>
      <Button
        type="submit"
        variant="secondary"
        aria-label={sendLabel}
        className="rounded-full bg-background px-4 shadow-xs hover:bg-background/80 dark:bg-input/40"
        disabled={!draft.text.trim() || sending}
      >
        Send
      </Button>
    </form>
  );
}
