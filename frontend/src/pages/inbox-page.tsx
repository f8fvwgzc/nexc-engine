import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CheckCheckIcon, InboxIcon } from 'lucide-react';
import { useNavigate } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { inboxQuery, markInboxRead } from '@/features/issues/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import type { Notification } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

/** What the actor did to the issue. */
const VERB: Record<Notification['kind'], string> = {
  assigned: 'assigned you',
  comment: 'commented on',
  state: 'moved',
  // Nobody did this one: the day arrived.
  due: 'is due:',
};

function List({ workspace }: { workspace: Workspace }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data: inbox, isPending, error } = useQuery(inboxQuery(workspace.id));
  const read = useMutation({
    mutationFn: (ids?: string[]) => markInboxRead(workspace.id, ids),
    onSuccess: (next) => queryClient.setQueryData(qk.issues.inbox(workspace.id), next),
  });
  if (isPending) {
    return (
      <div className="space-y-2" aria-busy>
        {[0, 1, 2, 3].map((i) => (
          <Skeleton key={i} className="h-10 w-full" />
        ))}
      </div>
    );
  }
  if (error) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {errorMessage(error)}
      </p>
    );
  }
  if (inbox.length === 0) {
    return (
      <EmptyState
        icon={InboxIcon}
        title="Nothing for you yet"
        description="You hear here when an issue is assigned to you, when someone comments on or moves an issue you created or are assigned, and when an issue of yours is due."
      />
    );
  }
  const unread = inbox.filter((n) => n.read_at === null).length;
  const open = (n: Notification) => {
    if (n.read_at === null) read.mutate([n.id]);
    void navigate(`/app/issues?issue=${n.issue.id}`);
  };
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between text-[13px] text-muted-foreground">
        <span>{unread === 0 ? 'All read' : `${unread} unread`}</span>
        <Button
          variant="ghost"
          size="sm"
          disabled={unread === 0 || read.isPending}
          onClick={() => read.mutate(undefined)}
        >
          <CheckCheckIcon />
          Mark all read
        </Button>
      </div>
      <ol className="divide-y rounded-lg border">
        {inbox.map((n) => (
          <li key={n.id}>
            <button
              type="button"
              onClick={() => open(n)}
              className="flex w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] transition-colors outline-none hover:bg-muted/60 focus-visible:bg-muted/60"
            >
              <span
                aria-label={n.read_at === null ? 'Unread' : undefined}
                className={`size-1.5 shrink-0 rounded-full ${n.read_at === null ? 'bg-primary' : 'bg-transparent'}`}
              />
              <PersonGlyph name={n.kind === 'due' ? 'Reminder' : n.actor?.name} />
              <span
                className={`min-w-0 flex-1 truncate ${n.read_at === null ? '' : 'text-muted-foreground'}`}
              >
                <span className="font-medium">
                  {n.kind === 'due' ? 'Your issue' : (n.actor?.name ?? 'Someone who left')}
                </span>{' '}
                {VERB[n.kind]} <span className="font-mono text-xs">{n.issue.identifier}</span>{' '}
                {n.issue.title}
              </span>
              <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
                {formatRelative(n.created_at)}
              </span>
            </button>
          </li>
        ))}
      </ol>
    </div>
  );
}

function Inbox() {
  const { current } = useCurrentWorkspace();
  if (!current) return <PageSkeleton />;
  return (
    <div className="mx-auto w-full max-w-4xl space-y-6 p-4 sm:p-6">
      <PageHeader title="Inbox" description={`What happened to your issues in ${current.name}.`} />
      <List key={current.id} workspace={current} />
    </div>
  );
}

export default function InboxPage() {
  return (
    <>
      <Seo title="Inbox" noIndex />
      <Inbox />
    </>
  );
}
