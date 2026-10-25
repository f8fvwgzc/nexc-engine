import { SmilePlusIcon } from 'lucide-react';
import { useState, type ComponentProps, type ReactElement } from 'react';

import { IconButton } from '@/components/custom-ui/icon-button';
import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { BubbleReactions } from '@/components/ui/bubble';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { cn } from '@/lib/utils';
import { REACTION_EMOJI, type Reaction } from '@/schemas/issue';

import { emojiName, reactionSummary, reactorsLabel } from './reactions';

/**
 * The allowed emoji in a small grid, opened from `children` (the button). `picked` marks the ones
 * already chosen. A pick closes it; the focus goes back to the button unless `onCloseAutoFocus`
 * says otherwise.
 */
export function EmojiPicker({
  label,
  onPick,
  picked,
  align = 'start',
  onCloseAutoFocus,
  children,
}: {
  label: string;
  onPick: (emoji: string) => void;
  picked?: ReadonlySet<string>;
  align?: ComponentProps<typeof PopoverContent>['align'];
  onCloseAutoFocus?: ComponentProps<typeof PopoverContent>['onCloseAutoFocus'];
  children: ReactElement;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent
        align={align}
        className="w-auto gap-0 p-1.5"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <div role="group" aria-label={label} className="grid grid-cols-5 gap-0.5">
          {REACTION_EMOJI.map((emoji) => (
            <button
              key={emoji}
              type="button"
              aria-label={emojiName(emoji)}
              aria-pressed={picked ? picked.has(emoji) : undefined}
              title={emojiName(emoji)}
              className="flex size-8 items-center justify-center rounded-md text-lg leading-none transition-[background-color,transform] outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50 aria-pressed:bg-sky-500/15 motion-safe:hover:scale-110"
              onClick={() => {
                onPick(emoji);
                setOpen(false);
              }}
            >
              {emoji}
            </button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}

/** The `+` that adds a reaction to an entry. */
export function AddReaction({
  reactions,
  onToggle,
  align,
  className,
}: {
  reactions: Reaction[];
  onToggle: (emoji: string, on: boolean) => void;
  align?: ComponentProps<typeof EmojiPicker>['align'];
  className?: string;
}) {
  const mine = new Set(reactions.filter((r) => r.reacted).map((r) => r.emoji));
  return (
    <EmojiPicker
      label="Pick a reaction"
      picked={mine}
      align={align}
      onPick={(e) => onToggle(e, !mine.has(e))}
    >
      <IconButton
        label="Add a reaction"
        size="icon-xs"
        className={cn('rounded-full text-muted-foreground hover:text-foreground', className)}
      >
        <SmilePlusIcon />
      </IconButton>
    </EmojiPicker>
  );
}

/**
 * The reactions to an entry as chips on one soft bubble: each emoji with its count, tinted when
 * the reader is among them, a click adding the reader's or taking it back; then the `+` for
 * another. Nothing while there are none (the entry's own `+` adds the first).
 */
export function ReactionBar({
  reactions,
  me,
  onToggle,
  className,
}: {
  reactions: Reaction[];
  me: string | undefined;
  onToggle: (emoji: string, on: boolean) => void;
  className?: string;
}) {
  if (reactions.length === 0) return null;
  return (
    <BubbleReactions
      side="bottom"
      align="start"
      className={cn(
        'static mt-2 translate-y-0 flex-wrap justify-start gap-0.5 bg-muted/70 ring-0 has-[button]:p-0.5 dark:bg-muted/60',
        className,
      )}
    >
      {reactions.map((r) => (
        <button
          key={r.emoji}
          type="button"
          aria-pressed={r.reacted}
          aria-label={`${r.emoji} ${r.count}`}
          title={reactorsLabel(r, me)}
          onClick={() => onToggle(r.emoji, !r.reacted)}
          className="inline-flex h-6 items-center gap-1 rounded-full px-2 text-xs font-medium text-muted-foreground tabular-nums transition-colors outline-none hover:bg-background/80 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 aria-pressed:bg-sky-500/15 aria-pressed:text-sky-700 aria-pressed:ring-1 aria-pressed:ring-sky-500/40 dark:aria-pressed:text-sky-300"
        >
          <span aria-hidden className="text-sm leading-none">
            {r.emoji}
          </span>
          {r.count}
        </button>
      ))}
      <AddReaction reactions={reactions} onToggle={onToggle} className="size-6" />
    </BubbleReactions>
  );
}

/** Who reacted to an entry: their faces, overlapping, and "Dana and 3 others reacted". */
export function ReactionSummary({
  reactions,
  me,
}: {
  reactions: Reaction[];
  me: string | undefined;
}) {
  const { people, text } = reactionSummary(reactions, me);
  if (!text) return null;
  return (
    <p className="flex min-h-6 min-w-0 items-center gap-2 text-[13px] text-muted-foreground">
      <span aria-hidden className="flex shrink-0 -space-x-1">
        {people.slice(0, 5).map((p) => (
          <PersonGlyph
            key={p.user_id}
            name={p.name}
            className="size-5 bg-muted text-[8px] ring-2 ring-background"
          />
        ))}
      </span>
      <span className="truncate">{text}</span>
    </p>
  );
}
