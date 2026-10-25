import { BotIcon } from 'lucide-react';
import {
  Fragment,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentProps,
  type KeyboardEvent,
} from 'react';
import { Link } from 'react-router-dom';

import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { Badge } from '@/components/ui/badge';
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card';
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover';
import { Textarea } from '@/components/ui/textarea';
import { cn } from '@/lib/utils';
import { useAuthStore } from '@/stores/auth-store';

import {
  addPicked,
  insertMention,
  matchMentionables,
  mentionKey,
  mentionQuery,
  pickedLabel,
  splitMentions,
  type MentionDraft,
  type Mentionable,
} from './mentions';
import type { MentionCandidate, Mentionables } from './use-mentionables';

type MentionTextareaProps = Omit<ComponentProps<'textarea'>, 'value' | 'onChange'> & {
  /** The text, and the mentions picked into it. */
  draft: MentionDraft;
  onDraftChange: (draft: MentionDraft) => void;
  candidates: MentionCandidate[];
  /** Cmd/Ctrl+Enter. */
  onSubmit?: () => void;
  /** A line under the list's agents. */
  agentNote?: string;
};

/**
 * A comment field where `@` opens a list of the members and agents the comment can mention,
 * narrowed as one types. Arrow keys move through it, Enter or Tab picks, Escape closes it; the
 * focus stays in the field. A pick writes `@Name` into the text and adds the mention to the draft,
 * so the comment can be sent with its tokens (`toWire`). Cmd/Ctrl+Enter sends.
 */
export function MentionTextarea({
  draft,
  onDraftChange,
  candidates,
  onSubmit,
  agentNote,
  onKeyDown,
  onBlur,
  ...props
}: MentionTextareaProps) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const listId = useId();
  // Where the caret is while nothing is selected; null when the field is not being typed in.
  const [caret, setCaret] = useState<number | null>(null);
  // The `@` whose list was closed (Escape, or a pick): it stays closed until another `@` is typed.
  const [dismissed, setDismissed] = useState<number | null>(null);
  const [active, setActive] = useState<string | null>(null);
  const moveCaretTo = useRef<number | null>(null);
  const value = draft.text;

  const query = caret === null ? null : mentionQuery(value, caret);
  const people = query
    ? matchMentionables(
        candidates.filter((c) => c.kind === 'user'),
        query.query,
        6,
      )
    : [];
  const agents = query
    ? matchMentionables(
        candidates.filter((c) => c.kind === 'agent'),
        query.query,
        5,
      )
    : [];
  const options = [...people, ...agents];
  const open = query !== null && query.start !== dismissed && options.length > 0;
  const current = options.find((o) => mentionKey(o) === active) ?? options[0];
  const optionId = (m: Mentionable) => `${listId}-${m.kind}-${m.id}`;

  const track = (field: HTMLTextAreaElement) => {
    const at = field.selectionStart === field.selectionEnd ? field.selectionStart : null;
    setCaret(at);
    if (at === null || mentionQuery(field.value, at) === null) setDismissed(null);
  };

  const pick = (m: MentionCandidate) => {
    if (!query || caret === null) return;
    const picked = addPicked(draft.picked, m, value);
    const next = insertMention(value, query.start, caret, pickedLabel(picked, m));
    moveCaretTo.current = next.caret;
    onDraftChange({ text: next.text, picked });
    setCaret(next.caret);
    setActive(null);
    // The name just written would match itself: the list stays shut until another `@`.
    setDismissed(query.start);
  };

  // After a pick, the caret goes right after the name it wrote.
  useLayoutEffect(() => {
    const at = moveCaretTo.current;
    if (at === null || !ref.current) return;
    moveCaretTo.current = null;
    ref.current.focus();
    ref.current.setSelectionRange(at, at);
  }, [value]);

  const currentId = open && current ? optionId(current) : undefined;
  useEffect(() => {
    if (currentId) document.getElementById(currentId)?.scrollIntoView?.({ block: 'nearest' });
  }, [currentId]);

  const keyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    onKeyDown?.(e);
    if (e.defaultPrevented || e.nativeEvent.isComposing) return;
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      onSubmit?.();
      return;
    }
    if (!open || !query || !current) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const step = e.key === 'ArrowDown' ? 1 : options.length - 1;
      const next = options[(options.indexOf(current) + step) % options.length];
      if (next) setActive(mentionKey(next));
    } else if ((e.key === 'Enter' || e.key === 'Tab') && !e.shiftKey) {
      e.preventDefault();
      pick(current);
    } else if (e.key === 'Escape') {
      // The list closes; the page does not take the key (it would leave the issue).
      e.preventDefault();
      e.stopPropagation();
      setDismissed(query.start);
    }
  };

  const option = (m: MentionCandidate) => {
    const selected = current !== undefined && mentionKey(current) === mentionKey(m);
    return (
      <div
        key={mentionKey(m)}
        id={optionId(m)}
        role="option"
        aria-selected={selected}
        data-selected={selected || undefined}
        // The focus stays in the field.
        onMouseDown={(e) => e.preventDefault()}
        onMouseMove={() => setActive(mentionKey(m))}
        onClick={() => pick(m)}
        className="flex cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm select-none data-selected:bg-muted"
      >
        <span aria-hidden className="shrink-0">
          {m.kind === 'user' ? (
            <PersonGlyph name={m.name} />
          ) : (
            <span className="flex size-5 items-center justify-center rounded-full bg-primary/10 text-primary">
              <BotIcon className="size-3.5" />
            </span>
          )}
        </span>
        <span className="min-w-0 flex-1 truncate">
          {m.name}
          {m.detail && <span className="ml-1.5 text-xs text-muted-foreground">{m.detail}</span>}
        </span>
        {m.kind === 'agent' && (
          <Badge variant="secondary" className="h-4 px-1.5 text-[10px]">
            Agent
          </Badge>
        )}
      </div>
    );
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        if (!next && query) setDismissed(query.start);
      }}
    >
      <PopoverAnchor asChild>
        <Textarea
          ref={ref}
          value={value}
          aria-autocomplete="list"
          aria-controls={open ? listId : undefined}
          aria-activedescendant={currentId}
          onChange={(e) => {
            onDraftChange({ ...draft, text: e.target.value });
            track(e.target);
          }}
          onSelect={(e) => track(e.currentTarget)}
          onKeyDown={keyDown}
          onBlur={(e) => {
            setCaret(null);
            onBlur?.(e);
          }}
          {...props}
        />
      </PopoverAnchor>
      <PopoverContent
        align="start"
        sideOffset={6}
        className="w-72 gap-0 p-1"
        onOpenAutoFocus={(e) => e.preventDefault()}
        onCloseAutoFocus={(e) => e.preventDefault()}
        onInteractOutside={(e) => {
          // A click back in the field keeps the list: the caret decides.
          if (e.target instanceof Node && ref.current?.contains(e.target)) e.preventDefault();
        }}
      >
        <div
          id={listId}
          role="listbox"
          aria-label="Mention a teammate or an agent"
          className="max-h-72 overflow-y-auto"
          // Scrolling the list keeps the focus, and the caret, in the field.
          onMouseDown={(e) => e.preventDefault()}
        >
          {people.length > 0 && (
            <div role="group" aria-label="People">
              <div aria-hidden className="px-2 py-1.5 text-xs font-medium text-muted-foreground">
                People
              </div>
              {people.map(option)}
            </div>
          )}
          {agents.length > 0 && (
            <div role="group" aria-label="Agents">
              <div aria-hidden className="px-2 py-1.5 text-xs font-medium text-muted-foreground">
                Agents
              </div>
              {agents.map(option)}
            </div>
          )}
        </div>
        {agents.length > 0 && agentNote && (
          <p className="px-2 pt-1 pb-1.5 text-xs text-muted-foreground">{agentNote}</p>
        )}
      </PopoverContent>
    </Popover>
  );
}

const CHIP =
  'inline-flex max-w-full items-center gap-1 rounded bg-muted px-1 align-baseline text-[13px] leading-5 font-medium';

/** What showing a comment's mentions needs: the live names, and whether they have loaded. */
type KnownMentionables = Pick<Mentionables, 'people' | 'agents' | 'peopleLoaded' | 'agentsLoaded'>;

/**
 * A mention in a comment: a member's name with a card on hover, or a link to the agent. One that
 * names nobody the reader knows of (once they have loaded) is plain `@Name`, as the server treats it.
 */
function MentionChip({
  mention,
  mentionables,
  me,
}: {
  mention: Mentionable;
  mentionables: KnownMentionables;
  me: string | undefined;
}) {
  const agent = mention.kind === 'agent';
  const known = agent ? mentionables.agents.has(mention.id) : mentionables.people.has(mention.id);
  if (!known && (agent ? mentionables.agentsLoaded : mentionables.peopleLoaded))
    return <>@{mention.name}</>;
  if (agent) {
    const name = mentionables.agents.get(mention.id) ?? mention.name;
    return (
      <Link
        to={`/app/agents?agent=${mention.id}`}
        className={cn(CHIP, 'text-foreground hover:bg-muted/70 hover:underline')}
      >
        <BotIcon aria-hidden className="size-3.5 shrink-0 text-primary" />
        <span className="truncate">{name}</span>
      </Link>
    );
  }
  const person = mentionables.people.get(mention.id);
  const name = person?.name ?? mention.name;
  return (
    <HoverCard openDelay={300} closeDelay={100}>
      <HoverCardTrigger asChild>
        <span className={cn(CHIP, mention.id === me && 'bg-brand/15')}>
          <span aria-hidden className="shrink-0">
            <PersonGlyph name={name} className="size-4 text-[8px]" />
          </span>
          <span className="truncate">{name}</span>
        </span>
      </HoverCardTrigger>
      <HoverCardContent align="start" className="w-auto max-w-72">
        <div className="flex items-center gap-2">
          <PersonGlyph name={name} className="size-8 text-xs" />
          <div className="min-w-0">
            <p className="truncate font-medium">{name}</p>
            {person?.email && (
              <p className="truncate text-xs text-muted-foreground">{person.email}</p>
            )}
          </div>
        </div>
      </HoverCardContent>
    </HoverCard>
  );
}

/** A comment's text with its mentions shown as chips; the rest as written. */
export function MentionText({
  body,
  mentionables,
  className,
}: {
  body: string;
  mentionables: KnownMentionables;
  className?: string;
}) {
  const me = useAuthStore((s) => s.user?.id);
  return (
    <p className={cn('text-sm break-words whitespace-pre-wrap', className)}>
      {splitMentions(body).map((segment, i) => (
        <Fragment key={i}>
          {segment.kind === 'text' ? (
            segment.text
          ) : (
            <MentionChip mention={segment.mention} mentionables={mentionables} me={me} />
          )}
        </Fragment>
      ))}
    </p>
  );
}
