import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ArrowUpIcon,
  HistoryIcon,
  MapPinIcon,
  SparklesIcon,
  SquarePenIcon,
  XIcon,
} from 'lucide-react';
import { AnimatePresence, MotionConfig, motion } from 'motion/react';
import { useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';

import { Markdown } from '@/components/custom-ui/markdown';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { askAssistant, conversationQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { ConversationMessage, TurnOutcome } from '@/schemas/assistant';
import type { Workspace } from '@/schemas/workspace';
import { useAssistantStore } from '@/stores/assistant-store';

import { usePageContext } from './use-page-context';

type Turn =
  { role: 'user'; content: string } | { role: 'assistant'; content: string; outcome: TurnOutcome };

/** Openers shown on an empty conversation; sent as-is when clicked. */
const SUGGESTIONS = [
  'What has this workspace learned recently?',
  'What is in progress right now?',
  'Create an issue for …',
];

/** Width of the docked panel; the page keeps the rest. On a phone it takes the whole width. */
const PANEL_WIDTH = 448;

const spring = { type: 'spring', stiffness: 420, damping: 34, mass: 0.8 } as const;
const rise = {
  initial: { opacity: 0, y: 10 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -4, transition: { duration: 0.12 } },
  transition: spring,
};

function fromMessages(messages: ConversationMessage[]): Turn[] {
  return messages.map((m) =>
    m.role === 'user'
      ? { role: 'user', content: m.content }
      : {
          role: 'assistant',
          content: m.content,
          outcome: m.outcome ?? { created: [], skipped: [], memories_used: 0 },
        },
  );
}

/**
 * The assistant every member has in every workspace: a notch on the right edge that docks a
 * full-height panel beside the page, so the page stays visible and usable while they talk. It
 * answers from the workspace's memory, knows which page the member is on, and files issues when
 * asked to. Conversations are saved; the Conversations page lists them to continue.
 */
export function AssistantDot() {
  const { current } = useCurrentWorkspace();
  // Keyed by workspace: a conversation belongs to the workspace it happened in.
  return current ? <AssistantPanel key={current.id} current={current} /> : null;
}

function AssistantPanel({ current }: { current: Workspace }) {
  const queryClient = useQueryClient();
  const workspaceId = current.id;
  const open = useAssistantStore((s) => s.open);
  const setOpen = useAssistantStore((s) => s.setOpen);
  const startNew = useAssistantStore((s) => s.startNew);
  const continueIn = useAssistantStore((s) => s.continueIn);
  // A conversation remembered for another workspace is not this panel's.
  const conversationId = useAssistantStore((s) =>
    s.workspaceId === workspaceId ? s.conversationId : null,
  );
  const page = usePageContext();
  // What was said in this session, for the conversation it belongs to (null until the first
  // reply names one). A saved conversation shows its messages until this session adds to it.
  const [thread, setThread] = useState<{ id: string | null; turns: Turn[] }>({
    id: null,
    turns: [],
  });
  const [text, setText] = useState('');
  const endRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const { data: saved, isPending: loadingSaved } = useQuery({
    ...conversationQuery(workspaceId, conversationId ?? ''),
    enabled: conversationId !== null,
  });
  const own = thread.id === conversationId;
  const loading = conversationId !== null && !own && loadingSaved;
  const turns: Turn[] = own
    ? thread.turns
    : saved && saved.conversation.id === conversationId
      ? fromMessages(saved.messages)
      : [];

  useEffect(() => {
    // Braces matter: newer browsers return a promise from scrollIntoView, and an effect may
    // only return a clean-up function.
    endRef.current?.scrollIntoView({ block: 'end', behavior: 'smooth' });
  }, [turns.length, open]);
  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open, conversationId]);

  const ask = useMutation({
    mutationFn: (message: string) => askAssistant(workspaceId, message, { conversationId, page }),
    meta: { errorToast: false },
    onSuccess: (reply) => {
      const outcome: TurnOutcome = {
        created: reply.created.map(({ id, identifier, title }) => ({ id, identifier, title })),
        skipped: reply.skipped,
        memories_used: reply.memories_used,
      };
      setThread((t) => ({
        id: reply.conversation_id,
        turns: [...t.turns, { role: 'assistant', content: reply.reply, outcome }],
      }));
      continueIn(workspaceId, reply.conversation_id);
      void queryClient.invalidateQueries({ queryKey: qk.workspaces.conversations(workspaceId) });
      if (reply.created.length > 0) {
        void queryClient.invalidateQueries({ queryKey: qk.issues.all });
      }
    },
  });
  const send = (draft = text) => {
    const message = draft.trim();
    if (!message || ask.isPending) return;
    setThread({ id: conversationId, turns: [...turns, { role: 'user', content: message }] });
    setText('');
    ask.mutate(message);
  };
  // A suggestion that ends in an ellipsis is a prompt to complete, not a message to send.
  const pick = (suggestion: string) => {
    if (suggestion.endsWith('…')) {
      setText(suggestion.slice(0, -1));
      inputRef.current?.focus();
    } else {
      send(suggestion);
    }
  };
  const fresh = () => {
    ask.reset();
    setText('');
    startNew();
    setThread({ id: null, turns: [] });
    inputRef.current?.focus();
  };

  return (
    <MotionConfig reducedMotion="user">
      {/* The notch sits on the right edge, halfway down, and slides away while the panel is open. */}
      <div className="pointer-events-none fixed inset-y-0 right-0 z-40 flex items-center">
        <AnimatePresence initial={false}>
          {!open && (
            <motion.button
              key="notch"
              type="button"
              initial={{ x: 40, opacity: 0 }}
              animate={{ x: 0, opacity: 1 }}
              exit={{ x: 40, opacity: 0 }}
              whileHover={{ width: 32 }}
              transition={spring}
              className="pointer-events-auto flex h-28 w-7 flex-col items-center justify-center gap-1.5 rounded-l-xl bg-primary text-primary-foreground shadow-lg outline-none focus-visible:ring-2 focus-visible:ring-ring"
              aria-label="Open assistant"
              aria-expanded={false}
              onClick={() => setOpen(true)}
            >
              <SparklesIcon className="size-3.5" aria-hidden />
              <span className="text-[11px] font-medium tracking-wide [writing-mode:vertical-rl]">
                Ask
              </span>
            </motion.button>
          )}
        </AnimatePresence>
      </div>

      {/* Docked beside the page: a column in the shell's row, not an overlay on top of it. */}
      <AnimatePresence initial={false}>
        {open && (
          <motion.aside
            key="panel"
            aria-label="Assistant"
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: PANEL_WIDTH, opacity: 1 }}
            exit={{ width: 0, opacity: 0 }}
            transition={spring}
            className="h-svh max-w-full shrink-0 overflow-hidden border-l bg-background text-sm"
            onKeyDown={(e) => {
              if (e.key === 'Escape') setOpen(false);
            }}
          >
            <div className="flex h-full max-w-full flex-col" style={{ width: PANEL_WIDTH }}>
              <header className="flex items-start gap-1 border-b py-3 pr-2 pl-4">
                <div className="min-w-0 flex-1">
                  <h2 className="flex items-center gap-2 font-heading text-sm font-medium">
                    <SparklesIcon className="size-4" aria-hidden />
                    <span className="min-w-0 truncate">Assistant · {current.name}</span>
                  </h2>
                  <p
                    className="mt-0.5 flex items-center gap-1 text-xs text-muted-foreground"
                    title={page.path}
                  >
                    <MapPinIcon className="size-3 shrink-0" aria-hidden />
                    <span className="min-w-0 truncate">Sees: {page.title || 'this page'}</span>
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="New conversation"
                  title="New conversation"
                  onClick={fresh}
                >
                  <SquarePenIcon />
                </Button>
                <Button variant="ghost" size="icon-sm" asChild>
                  <Link
                    to="/app/conversations"
                    aria-label="Past conversations"
                    title="Past conversations"
                  >
                    <HistoryIcon />
                  </Link>
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Close assistant"
                  onClick={() => setOpen(false)}
                >
                  <XIcon />
                </Button>
              </header>

              <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
                <div className="flex flex-col gap-3">
                  {loading && (
                    <p className="py-6 text-muted-foreground">Loading the conversation…</p>
                  )}
                  {!loading && turns.length === 0 && (
                    <motion.div {...rise} className="flex flex-col gap-3 py-6">
                      <div className="flex size-10 items-center justify-center rounded-full bg-primary/10 text-primary">
                        <SparklesIcon className="size-5" aria-hidden />
                      </div>
                      <p className="text-base font-medium">How can I help in {current.name}?</p>
                      <p className="text-muted-foreground">
                        Ask about what this workspace has learned or about the page you are on, or
                        say “create an issue for …” and it files one with a team you can file with.
                      </p>
                      <ul className="flex flex-wrap gap-2" aria-label="Suggestions">
                        {SUGGESTIONS.map((suggestion) => (
                          <li key={suggestion}>
                            <button
                              type="button"
                              className="rounded-full border px-3 py-1.5 text-xs transition-colors hover:bg-muted"
                              onClick={() => pick(suggestion)}
                            >
                              {suggestion}
                            </button>
                          </li>
                        ))}
                      </ul>
                    </motion.div>
                  )}
                  <AnimatePresence initial={false}>
                    {turns.map((turn, i) =>
                      turn.role === 'user' ? (
                        <motion.p
                          key={i}
                          {...rise}
                          className="ml-10 self-end rounded-2xl rounded-br-md bg-primary px-3.5 py-2 whitespace-pre-wrap text-primary-foreground"
                        >
                          {turn.content}
                        </motion.p>
                      ) : (
                        <motion.div key={i} {...rise} className="mr-6 flex gap-2.5">
                          <span
                            className="mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary"
                            aria-hidden
                          >
                            <SparklesIcon className="size-3.5" />
                          </span>
                          <div className="min-w-0 space-y-2">
                            <Markdown className="text-sm">{turn.content}</Markdown>
                            {turn.outcome.created.map((issue) => (
                              <Link
                                key={issue.id}
                                to="/app/issues"
                                className="flex items-center gap-2 rounded-lg border px-2.5 py-1.5 hover:bg-muted/50"
                              >
                                <span className="font-mono text-xs text-muted-foreground">
                                  {issue.identifier}
                                </span>
                                <span className="min-w-0 flex-1 truncate">{issue.title}</span>
                              </Link>
                            ))}
                            {turn.outcome.skipped.length > 0 && (
                              <p className="text-xs text-muted-foreground">
                                Not filed: {turn.outcome.skipped.join('; ')}
                              </p>
                            )}
                            {turn.outcome.memories_used > 0 && (
                              <p className="text-[11px] text-muted-foreground">
                                From {turn.outcome.memories_used}{' '}
                                {turn.outcome.memories_used === 1 ? 'memory' : 'memories'}
                              </p>
                            )}
                          </div>
                        </motion.div>
                      ),
                    )}
                    {ask.isPending && (
                      <motion.div
                        key="thinking"
                        {...rise}
                        className="mr-6 flex items-center gap-2.5"
                        role="status"
                        aria-label="Thinking"
                      >
                        <span
                          className="flex size-6 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary"
                          aria-hidden
                        >
                          <SparklesIcon className="size-3.5" />
                        </span>
                        <span className="flex gap-1" aria-hidden>
                          {[0, 1, 2].map((n) => (
                            <motion.span
                              key={n}
                              className="size-1.5 rounded-full bg-muted-foreground"
                              animate={{ opacity: [0.3, 1, 0.3] }}
                              transition={{ duration: 1, repeat: Infinity, delay: n * 0.15 }}
                            />
                          ))}
                        </span>
                      </motion.div>
                    )}
                  </AnimatePresence>
                  {ask.error && (
                    <p role="alert" className="text-destructive">
                      {errorMessage(ask.error)}
                    </p>
                  )}
                  <div ref={endRef} />
                </div>
              </div>

              <form
                className="border-t px-4 py-3"
                onSubmit={(e) => {
                  e.preventDefault();
                  send();
                }}
              >
                <div className="flex items-end gap-2 rounded-xl border bg-background p-1.5 transition-colors focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50">
                  <Textarea
                    ref={inputRef}
                    value={text}
                    rows={1}
                    maxLength={4000}
                    placeholder="Ask, or ask it to file an issue…"
                    aria-label="Message to the assistant"
                    onChange={(e) => setText(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && !e.shiftKey) {
                        e.preventDefault();
                        send();
                      }
                    }}
                    className="max-h-40 min-h-9 resize-none border-0 bg-transparent px-2 py-1.5 shadow-none focus-visible:ring-0 dark:bg-transparent"
                  />
                  <Button
                    type="submit"
                    size="icon-sm"
                    className="rounded-lg"
                    disabled={!text.trim() || ask.isPending}
                    aria-label="Send"
                  >
                    <ArrowUpIcon />
                  </Button>
                </div>
                <p className="mt-1.5 text-[11px] text-muted-foreground">
                  Enter to send · Shift+Enter for a new line
                </p>
              </form>
            </div>
          </motion.aside>
        )}
      </AnimatePresence>
    </MotionConfig>
  );
}
