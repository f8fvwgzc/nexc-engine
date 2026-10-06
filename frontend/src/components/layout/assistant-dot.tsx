import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ArrowUpIcon, SparklesIcon, XIcon } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';

import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { askAssistant } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { AssistantReply } from '@/schemas/assistant';
import type { Workspace } from '@/schemas/workspace';

type Turn =
  { role: 'user'; content: string } | { role: 'assistant'; content: string; reply: AssistantReply };

/**
 * The assistant every member has in every workspace: a dot in the corner that opens a chat.
 * It answers from the workspace's memory and files issues when asked to.
 */
export function AssistantDot() {
  const { current } = useCurrentWorkspace();
  // Keyed by workspace: a conversation belongs to the workspace it happened in.
  return current ? <AssistantPanel key={current.id} current={current} /> : null;
}

function AssistantPanel({ current }: { current: Workspace }) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [text, setText] = useState('');
  const endRef = useRef<HTMLDivElement>(null);
  const workspaceId = current.id;

  useEffect(() => {
    // Braces matter: newer browsers return a promise from scrollIntoView, and an effect may
    // only return a clean-up function.
    endRef.current?.scrollIntoView({ block: 'end' });
  }, [turns, open]);

  const ask = useMutation({
    mutationFn: (message: string) =>
      askAssistant(
        workspaceId,
        message,
        turns.map(({ role, content }) => ({ role, content })),
      ),
    meta: { errorToast: false },
    onSuccess: (reply) => {
      setTurns((t) => [...t, { role: 'assistant', content: reply.reply, reply }]);
      if (reply.created.length > 0) {
        void queryClient.invalidateQueries({ queryKey: qk.issues.all });
      }
    },
  });
  const send = () => {
    const message = text.trim();
    if (!message || ask.isPending) return;
    setTurns((t) => [...t, { role: 'user', content: message }]);
    setText('');
    ask.mutate(message);
  };

  return (
    <div className="fixed right-4 bottom-4 z-40 flex flex-col items-end gap-3">
      {open && (
        <section
          aria-label="Assistant"
          className="flex h-[min(32rem,70svh)] w-[min(24rem,calc(100vw-2rem))] flex-col overflow-hidden rounded-xl border bg-background shadow-lg motion-safe:animate-fade-in"
        >
          <header className="flex items-center gap-2 border-b px-3 py-2">
            <SparklesIcon className="size-4" aria-hidden />
            <h2 className="min-w-0 flex-1 truncate text-sm font-medium">
              Assistant · {current.name}
            </h2>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Close assistant"
              onClick={() => setOpen(false)}
            >
              <XIcon />
            </Button>
          </header>
          <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-3 text-sm">
            {turns.length === 0 && (
              <p className="text-muted-foreground">
                Ask about what this workspace has learned, or say “create an issue for …” and it
                files one with a team you can file with.
              </p>
            )}
            {turns.map((turn, i) =>
              turn.role === 'user' ? (
                <p key={i} className="ml-8 rounded-lg bg-muted px-3 py-2 whitespace-pre-wrap">
                  {turn.content}
                </p>
              ) : (
                <div key={i} className="mr-8 space-y-2">
                  <p className="whitespace-pre-wrap">{turn.content}</p>
                  {turn.reply.created.map((issue) => (
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
                  {turn.reply.skipped.length > 0 && (
                    <p className="text-xs text-muted-foreground">
                      Not filed: {turn.reply.skipped.join('; ')}
                    </p>
                  )}
                </div>
              ),
            )}
            {ask.isPending && <p className="text-muted-foreground">Thinking…</p>}
            {ask.error && (
              <p role="alert" className="text-destructive">
                {errorMessage(ask.error)}
              </p>
            )}
            <div ref={endRef} />
          </div>
          <form
            className="flex items-end gap-2 border-t p-2"
            onSubmit={(e) => {
              e.preventDefault();
              send();
            }}
          >
            <Textarea
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
              className="max-h-32 min-h-9 resize-none"
            />
            <Button
              type="submit"
              size="icon-sm"
              disabled={!text.trim() || ask.isPending}
              aria-label="Send"
            >
              <ArrowUpIcon />
            </Button>
          </form>
        </section>
      )}
      <Button
        size="icon"
        className="size-11 rounded-full shadow-lg"
        aria-label={open ? 'Hide assistant' : 'Open assistant'}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <SparklesIcon />
      </Button>
    </div>
  );
}
