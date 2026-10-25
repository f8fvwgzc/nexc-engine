import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CheckCheckIcon,
  CheckIcon,
  CircleDotIcon,
  InboxIcon,
  SirenIcon,
  UserCheckIcon,
  XIcon,
} from 'lucide-react';
import { useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Skeleton } from '@/components/ui/skeleton';
import { Textarea } from '@/components/ui/textarea';
import { inboxQuery, markInboxRead } from '@/features/issues/api';
import { issuePath } from '@/features/issues/links';
import { approveApproval, denyApproval } from '@/features/policies/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { displayPrincipal } from '@/schemas/agent';
import type { Notification } from '@/schemas/issue';
import { SEVERITY_META } from '@/schemas/policy';
import type { Workspace } from '@/schemas/workspace';

/** What the actor did to the issue. */
const VERB: Record<'assigned' | 'comment' | 'state' | 'due' | 'run' | 'mention', string> = {
  assigned: 'assigned you',
  comment: 'commented on',
  mention: 'mentioned you in',
  state: 'moved',
  // Nobody did this one: the day arrived.
  due: 'is due:',
  // Agents did this one: a run of the issue's graph finished and its result is on the issue.
  run: 'finished work on',
};

/** Who an issue entry names: nobody for a reminder, the agents for a run, else the actor. */
function entryActor(n: Notification): string {
  if (n.kind === 'due') return 'Your issue';
  if (n.kind === 'run') return 'Agents';
  return n.actor?.name ?? 'Someone who left';
}

const ROW =
  'flex w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] transition-colors outline-none hover:bg-muted/60 focus-visible:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset';

function Unread({ read }: { read: boolean }) {
  return (
    <span
      {...(read ? { 'aria-hidden': true } : { role: 'img', 'aria-label': 'Unread' })}
      className={`size-1.5 shrink-0 rounded-full ${read ? 'bg-transparent' : 'bg-primary'}`}
    />
  );
}

function IssueEntry({ n, onOpen }: { n: Notification; onOpen: (n: Notification) => void }) {
  if (!n.issue || n.kind === 'approval' || n.kind === 'alert') return null;
  const read = n.read_at !== null;
  return (
    <button type="button" onClick={() => onOpen(n)} className={ROW}>
      <Unread read={read} />
      <PersonGlyph
        name={n.kind === 'due' ? 'Reminder' : n.kind === 'run' ? 'Agents' : n.actor?.name}
      />
      <span
        className={`min-w-0 flex-1 truncate ${read ? 'text-muted-foreground' : ''}`}
        title={`${n.issue.identifier} ${n.issue.title}`}
      >
        <span className="font-medium">{entryActor(n)}</span> {VERB[n.kind]}{' '}
        <span className="font-mono text-xs">{n.issue.identifier}</span> {n.issue.title}
      </span>
      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
        {formatRelative(n.created_at)}
      </span>
    </button>
  );
}

interface DecideProps {
  n: Notification;
  workspaceId: string;
}

/** A held tool call: who asks, what for, and the two buttons. */
function ApprovalEntry({ n, workspaceId }: DecideProps) {
  const queryClient = useQueryClient();
  const [denying, setDenying] = useState(false);
  const [reason, setReason] = useState('');
  const approval = n.approval;
  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: qk.issues.inbox(workspaceId) });
    void queryClient.invalidateQueries({ queryKey: qk.approvals.all });
  };
  const approve = useMutation({
    mutationFn: () => approveApproval(approval?.id ?? '', ''),
    onSuccess: (a) => {
      toast.success(`${a.tool} approved: the run goes on`);
      refresh();
    },
    onError: (err) => toast.error(errorMessage(err)),
  });
  const deny = useMutation({
    mutationFn: () => denyApproval(approval?.id ?? '', reason),
    onSuccess: (a) => {
      toast.success(`${a.tool} denied: the agent was told why`);
      setDenying(false);
      refresh();
    },
    onError: (err) => toast.error(errorMessage(err)),
  });
  if (!approval) return null;
  const read = n.read_at !== null;
  const pending = approval.status === 'pending';
  return (
    // On a phone the request takes the first line and the buttons the second.
    <div className={`${ROW} cursor-default flex-wrap`}>
      <Unread read={read} />
      <UserCheckIcon className="size-4 shrink-0 text-muted-foreground" aria-hidden />
      <span
        className={`min-w-0 flex-1 basis-[calc(100%-2.75rem)] sm:basis-0 ${read ? 'text-muted-foreground' : ''}`}
      >
        <span className="font-medium">{displayPrincipal(approval.principal)}</span> asks to run{' '}
        <code className="rounded bg-muted px-1 py-0.5 text-xs">{approval.tool}</code>
        {approval.reason && (
          <span className="text-muted-foreground"> · {approval.reason}</span>
        )}{' '}
        <Link
          to={`/app/runs/${approval.run_id}`}
          className="text-xs text-muted-foreground hover:underline"
        >
          run {approval.run_id.slice(0, 8)}
        </Link>
        {!pending && (
          <Badge variant="outline" className="ml-2">
            {approval.status}
          </Badge>
        )}
      </span>
      {pending ? (
        <span className="ml-auto flex shrink-0 items-center gap-1">
          <AnimatedButton size="sm" loading={approve.isPending} onClick={() => approve.mutate()}>
            <CheckIcon />
            Approve
          </AnimatedButton>
          <Button size="sm" variant="outline" onClick={() => setDenying(true)}>
            <XIcon />
            Deny
          </Button>
        </span>
      ) : (
        <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
          {formatRelative(n.created_at)}
        </span>
      )}
      <Dialog open={denying} onOpenChange={setDenying}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Deny {approval.tool}?</DialogTitle>
            <DialogDescription>
              The agent sees your reason as the tool’s answer and goes on without it.
            </DialogDescription>
          </DialogHeader>
          <Textarea
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder="Why the call may not go ahead"
            aria-label="Reason"
          />
          <DialogFooter>
            <Button variant="outline" onClick={() => setDenying(false)}>
              Cancel
            </Button>
            <AnimatedButton
              variant="destructive"
              disabled={reason.trim().length === 0}
              loading={deny.isPending}
              onClick={() => deny.mutate()}
            >
              Deny
            </AnimatedButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function AlertEntry({ n, onOpen }: { n: Notification; onOpen: (n: Notification) => void }) {
  const alert = n.alert;
  if (!alert) return null;
  const read = n.read_at !== null;
  const meta = SEVERITY_META[alert.severity];
  return (
    <button type="button" onClick={() => onOpen(n)} className={ROW}>
      <Unread read={read} />
      <SirenIcon
        className="size-4 shrink-0"
        style={{ color: meta.color }}
        aria-label={`${meta.label} alert`}
      />
      <span
        className={`min-w-0 flex-1 truncate ${read ? 'text-muted-foreground' : ''}`}
        title={`${alert.title} · ${displayPrincipal(alert.principal)} · ${alert.rule}`}
      >
        <span className="font-medium">{alert.title}</span>
        <span className="text-muted-foreground">
          {' '}
          · {displayPrincipal(alert.principal)} · {alert.rule}
        </span>
      </span>
      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
        {formatRelative(n.created_at)}
      </span>
    </button>
  );
}

function List({ workspace }: { workspace: Workspace }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data: inbox, isPending, error, refetch } = useQuery(inboxQuery(workspace.id));
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
      <ErrorState title="Couldn’t load the inbox" error={error} onRetry={() => void refetch()} />
    );
  }
  if (inbox.length === 0) {
    return (
      <EmptyState
        icon={InboxIcon}
        title="Nothing for you yet"
        description="You hear here when an issue is assigned to you, when someone mentions you in a comment, when someone comments on or moves an issue you created or are assigned, when an issue of yours is due, when an agent's tool call waits for your approval, and when an agent needs a look."
        action={
          <Button variant="outline" asChild>
            <Link to="/app/issues">
              <CircleDotIcon />
              Go to issues
            </Link>
          </Button>
        }
      />
    );
  }
  const unread = inbox.filter((n) => n.read_at === null).length;
  const open = (n: Notification) => {
    if (n.read_at === null) read.mutate([n.id]);
    if (n.issue) void navigate(issuePath(n.issue.id));
    else if (n.alert) void navigate('/app/settings/alerts');
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
            {n.kind === 'approval' ? (
              <ApprovalEntry n={n} workspaceId={workspace.id} />
            ) : n.kind === 'alert' ? (
              <AlertEntry n={n} onOpen={open} />
            ) : (
              <IssueEntry n={n} onOpen={open} />
            )}
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
    <PageContainer>
      <PageHeader
        title="Inbox"
        description={`What happened to your issues in ${current.name}, and what waits for your decision.`}
      />
      <List key={current.id} workspace={current} />
    </PageContainer>
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
