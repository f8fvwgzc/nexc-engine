import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CheckIcon, UserCheckIcon, XIcon } from 'lucide-react';
import { useState } from 'react';
import { Link } from 'react-router-dom';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { approvalsQuery, approveApproval, denyApproval } from '@/features/policies/api';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { displayPrincipal } from '@/schemas/agent';
import type { Approval } from '@/schemas/policy';

import { Panel, PanelEmpty } from './console-panel';

/** The buttons that decide a held call, with the reason a denial needs, inline. */
export function DecideApproval({
  approval,
  workspaceId,
}: {
  approval: Approval;
  workspaceId: string;
}) {
  const queryClient = useQueryClient();
  const [denying, setDenying] = useState(false);
  const [reason, setReason] = useState('');
  const settle = (a: Approval) => {
    queryClient.setQueriesData<Approval[]>({ queryKey: qk.approvals.all }, (list) =>
      list?.map((x) => (x.id === a.id ? a : x)),
    );
    void queryClient.invalidateQueries({ queryKey: qk.approvals.all });
    void queryClient.invalidateQueries({ queryKey: qk.secops.overview(workspaceId) });
    void queryClient.invalidateQueries({ queryKey: qk.issues.inbox(workspaceId) });
  };
  const approve = useMutation({
    mutationFn: () => approveApproval(approval.id, ''),
    onSuccess: (a) => {
      toast.success(`${a.tool} approved: the run goes on`);
      settle(a);
    },
    onError: (err) => toast.error(errorMessage(err)),
  });
  const deny = useMutation({
    mutationFn: () => denyApproval(approval.id, reason.trim()),
    onSuccess: (a) => {
      toast.success(`${a.tool} denied: the agent was told why`);
      setDenying(false);
      settle(a);
    },
    onError: (err) => toast.error(errorMessage(err)),
  });
  if (denying) {
    return (
      <form
        className="flex flex-wrap items-center gap-1"
        onSubmit={(e) => {
          e.preventDefault();
          if (reason.trim()) deny.mutate();
        }}
      >
        <Input
          autoFocus
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          placeholder="Why not"
          aria-label="Reason for denying"
          className="h-7 w-40 text-xs"
          maxLength={2000}
        />
        <AnimatedButton
          size="xs"
          type="submit"
          variant="destructive"
          loading={deny.isPending}
          disabled={reason.trim().length === 0}
        >
          Deny
        </AnimatedButton>
        <Button size="xs" type="button" variant="ghost" onClick={() => setDenying(false)}>
          Cancel
        </Button>
      </form>
    );
  }
  return (
    <span className="flex items-center gap-1">
      <AnimatedButton size="xs" loading={approve.isPending} onClick={() => approve.mutate()}>
        <CheckIcon />
        Approve
      </AnimatedButton>
      <Button size="xs" variant="outline" onClick={() => setDenying(true)}>
        <XIcon />
        Deny
      </Button>
    </span>
  );
}

/** One held call: who asks, what for, and where. */
export function ApprovalRow({
  approval,
  workspaceId,
  decide,
}: {
  approval: Approval;
  workspaceId: string;
  decide: boolean;
}) {
  const pending = approval.status === 'pending';
  return (
    <li className="flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-2 text-xs">
      <span className="min-w-0 flex-1">
        <span className="font-medium">{displayPrincipal(approval.principal)}</span> asks to run{' '}
        <code className="rounded bg-muted px-1 py-0.5">{approval.tool}</code>
        {approval.reason && (
          <span className="text-muted-foreground"> · {approval.reason}</span>
        )}{' '}
        <Link
          to={`/app/runs/${approval.run_id}?span=${approval.span_id}`}
          className="text-muted-foreground hover:underline"
        >
          run {approval.run_id.slice(0, 8)}
        </Link>
        <span className="text-muted-foreground">
          {' '}
          · {pending ? `expires ${formatRelative(approval.expires_at)}` : approval.status}
          {approval.decided_by ? ` by ${approval.decided_by.name}` : ''}
        </span>
      </span>
      {pending && decide && <DecideApproval approval={approval} workspaceId={workspaceId} />}
    </li>
  );
}

interface ApprovalsPanelProps {
  workspaceId: string;
  admin: boolean;
  className?: string;
}

/** Tool calls waiting for a human, decided here. */
export function ApprovalsPanel({ workspaceId, admin, className }: ApprovalsPanelProps) {
  const { data, isPending, error } = useQuery({
    ...approvalsQuery(workspaceId, 'pending'),
    enabled: admin,
  });
  const waiting = data ?? [];
  return (
    <Panel
      id="secops-approvals"
      title="Approvals"
      icon={UserCheckIcon}
      count={admin ? waiting.length : undefined}
      className={className}
      bodyClassName="p-0"
    >
      {!admin ? (
        <PanelEmpty>Admins decide held tool calls; yours reach you in the Inbox.</PanelEmpty>
      ) : isPending ? (
        <div className="p-3">
          <Skeleton className="h-16 rounded-lg" />
        </div>
      ) : error ? (
        <p role="alert" className="p-3 text-xs text-destructive">
          {errorMessage(error)}
        </p>
      ) : waiting.length === 0 ? (
        <PanelEmpty>Nothing waits for a decision.</PanelEmpty>
      ) : (
        <ul className="max-h-72 divide-y overflow-y-auto" aria-label="Pending approvals">
          {waiting.map((approval) => (
            <ApprovalRow key={approval.id} approval={approval} workspaceId={workspaceId} decide />
          ))}
        </ul>
      )}
    </Panel>
  );
}
