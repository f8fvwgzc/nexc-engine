import { useMutation, useQueryClient, useSuspenseQuery } from '@tanstack/react-query';
import { format, formatDistanceToNow, isToday, isYesterday } from 'date-fns';
import { MapPinIcon, MessagesSquareIcon, SparklesIcon, Trash2Icon } from 'lucide-react';
import { Suspense } from 'react';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { PageHeader } from '@/components/custom-ui/page-header';
import { TableSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { conversationsQuery, deleteConversation } from '@/features/workspaces/api';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';
import { qk } from '@/lib/query-keys';
import type { Conversation } from '@/schemas/assistant';
import { useAssistantStore } from '@/stores/assistant-store';

/** The day a conversation was last continued, as a timeline heading. */
function dayOf(iso: string): string {
  const date = new Date(iso);
  if (isToday(date)) return 'Today';
  if (isYesterday(date)) return 'Yesterday';
  return format(date, 'EEEE, d MMMM yyyy');
}

function groupByDay(conversations: Conversation[]): { day: string; items: Conversation[] }[] {
  const groups: { day: string; items: Conversation[] }[] = [];
  for (const c of conversations) {
    const day = dayOf(c.updated_at);
    const last = groups[groups.length - 1];
    if (last && last.day === day) last.items.push(c);
    else groups.push({ day, items: [c] });
  }
  return groups;
}

function Timeline({ workspaceId }: { workspaceId: string }) {
  const queryClient = useQueryClient();
  const { data: conversations } = useSuspenseQuery(conversationsQuery(workspaceId));
  const show = useAssistantStore((s) => s.show);
  const shownId = useAssistantStore((s) =>
    s.workspaceId === workspaceId ? s.conversationId : null,
  );
  const startNew = useAssistantStore((s) => s.startNew);
  const openPanel = useAssistantStore((s) => s.setOpen);
  const remove = useMutation({
    mutationFn: (id: string) => deleteConversation(workspaceId, id),
    onSuccess: (_, id) => {
      if (id === shownId) startNew();
      void queryClient.invalidateQueries({ queryKey: qk.workspaces.conversations(workspaceId) });
    },
  });

  if (conversations.length === 0) {
    return (
      <EmptyState
        icon={MessagesSquareIcon}
        title="No conversations yet"
        description="Open the assistant from the notch on the right edge of any page and ask it something. Every conversation is kept here, to read back and continue."
        action={
          <Button
            onClick={() => {
              startNew();
              openPanel(true);
            }}
          >
            <SparklesIcon />
            Ask the assistant
          </Button>
        }
      />
    );
  }

  return (
    <ol className="relative space-y-8 border-l pl-6" aria-label="Conversations by day">
      {groupByDay(conversations).map((group) => (
        <li key={group.day}>
          <span
            className="absolute -left-1.25 mt-1.5 size-2.5 rounded-full border-2 border-background bg-primary"
            aria-hidden
          />
          <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
            {group.day}
          </h2>
          <ul className="mt-3 space-y-2">
            {group.items.map((c) => (
              <li
                key={c.id}
                className="group flex items-start gap-2 rounded-lg border bg-card p-3 transition-colors hover:bg-muted/40 has-[[data-open=true]]:border-primary"
              >
                <button
                  type="button"
                  className="min-w-0 flex-1 text-left outline-none"
                  data-open={c.id === shownId}
                  onClick={() => show(workspaceId, c.id)}
                  aria-label={`Continue “${c.title}”`}
                >
                  <p className="truncate font-medium">{c.title}</p>
                  <p className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
                    <span>
                      {c.message_count} {c.message_count === 1 ? 'message' : 'messages'}
                    </span>
                    {c.page_title && (
                      <span className="flex items-center gap-1" title={c.page_path}>
                        <MapPinIcon className="size-3" aria-hidden />
                        {c.page_title}
                      </span>
                    )}
                    <time dateTime={c.updated_at} title={format(new Date(c.updated_at), 'PPpp')}>
                      {formatDistanceToNow(new Date(c.updated_at), { addSuffix: true })}
                    </time>
                  </p>
                </button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className="shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
                  aria-label={`Delete “${c.title}”`}
                  disabled={remove.isPending}
                  onClick={() => remove.mutate(c.id)}
                >
                  <Trash2Icon />
                </Button>
              </li>
            ))}
          </ul>
        </li>
      ))}
    </ol>
  );
}

function ConversationsList() {
  const workspaceId = useWorkspaceId();
  return workspaceId ? <Timeline workspaceId={workspaceId} /> : <TableSkeleton />;
}

export default function ConversationsPage() {
  return (
    <>
      <Seo title="Conversations" noIndex />
      <div className="mx-auto w-full max-w-3xl space-y-6 p-4 sm:p-6">
        <PageHeader
          title="Conversations"
          description="Everything you have talked over with the assistant in this workspace, latest first. Open one to read it back and keep going."
        />
        <Suspense fallback={<TableSkeleton />}>
          <ConversationsList />
        </Suspense>
      </div>
    </>
  );
}
