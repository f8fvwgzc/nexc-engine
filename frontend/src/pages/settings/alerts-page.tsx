import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CheckIcon, GavelIcon, SirenIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { acknowledgeAlert, alertsQuery } from '@/features/policies/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { formatDateTime, formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { displayPrincipal } from '@/schemas/agent';
import { SeverityBadge } from '@/features/policies/components/severity-badge';
import type { Alert, AlertKind } from '@/schemas/policy';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

const KIND_LABEL: Record<AlertKind, string> = {
  dlp: 'Data leaving',
  risk: 'Behaviour',
  injection: 'Injection',
  policy: 'Policy',
};

function Feed({ workspace }: { workspace: Workspace }) {
  const queryClient = useQueryClient();
  const { data: alerts, isPending, error, refetch } = useQuery(alertsQuery(workspace.id));
  const acknowledge = useMutation({
    mutationFn: acknowledgeAlert,
    onSuccess: (seen) =>
      queryClient.setQueryData<Alert[]>(qk.alerts.list(workspace.id), (list) =>
        list?.map((a) => (a.id === seen.id ? seen : a)),
      ),
  });
  if (isPending) return <Skeleton className="h-48 rounded-xl" />;
  if (error) {
    return (
      <ErrorState title="Couldn’t load the alerts" error={error} onRetry={() => void refetch()} />
    );
  }
  if (alerts.length === 0) {
    return (
      <EmptyState
        icon={SirenIcon}
        title="Nothing to look at"
        description="A credential or personal data on its way out, a run unlike an agent's usual ones, or instructions planted in data would appear here. The policies set what raises one."
        action={
          <Button variant="outline" asChild>
            <Link to="/app/settings/policies">
              <GavelIcon />
              Review the policies
            </Link>
          </Button>
        }
      />
    );
  }
  const open = alerts.filter((a) => a.acknowledged_at === null).length;
  return (
    <div className="space-y-3">
      <p className="text-[13px] text-muted-foreground">
        {open === 0 ? 'All acknowledged' : `${open} to acknowledge`}
      </p>
      <ol className="divide-y rounded-lg border">
        {alerts.map((alert) => (
          <li
            key={alert.id}
            className={`flex flex-col gap-1 px-3 py-2.5 text-[13px] sm:flex-row sm:items-start sm:gap-3 ${alert.acknowledged_at ? 'text-muted-foreground' : ''}`}
          >
            <div className="flex shrink-0 items-center gap-2 pt-0.5">
              <SeverityBadge severity={alert.severity} />
              <Badge variant="outline">{KIND_LABEL[alert.kind]}</Badge>
            </div>
            <div className="min-w-0 flex-1 space-y-0.5">
              <p className="font-medium">{alert.title}</p>
              {alert.detail && <p className="text-xs text-muted-foreground">{alert.detail}</p>}
              <p className="flex flex-wrap gap-x-2 text-xs text-muted-foreground">
                <span title={alert.principal}>{displayPrincipal(alert.principal)}</span>
                <span className="font-mono">{alert.rule}</span>
                {alert.run_id && (
                  <Link
                    to={`/app/runs/${alert.run_id}`}
                    className="inline-flex min-h-[1.6rem] items-center hover:underline"
                  >
                    run {alert.run_id.slice(0, 8)}
                  </Link>
                )}
                <time dateTime={alert.created_at} title={formatDateTime(alert.created_at)}>
                  {formatRelative(alert.created_at)}
                </time>
                {alert.deliveries > 0 && (
                  <span title="Delivered to the workspace's alert destinations">
                    sent to {alert.deliveries} destination{alert.deliveries === 1 ? '' : 's'}
                  </span>
                )}
              </p>
            </div>
            <div className="shrink-0">
              {alert.acknowledged_at ? (
                <span className="text-xs">seen {formatRelative(alert.acknowledged_at)}</span>
              ) : (
                <Button
                  variant="outline"
                  size="sm"
                  disabled={acknowledge.isPending}
                  onClick={() => acknowledge.mutate(alert.id)}
                >
                  <CheckIcon />
                  Acknowledge
                </Button>
              )}
            </div>
          </li>
        ))}
      </ol>
    </div>
  );
}

export default function AlertsSettingsPage() {
  const { current } = useCurrentWorkspace();
  if (!current) return <PageSkeleton width="narrow" />;
  const allowed = isWorkspaceAdmin(current.role);
  return (
    <PageContainer width="narrow">
      <Seo title="Alerts" noIndex />
      <PageHeader
        title="Alerts"
        description={`What the agents of ${current.name} did that needs a look: data on its way out, behaviour off its baseline, instructions planted in data.`}
      />
      {allowed ? (
        <Feed key={current.id} workspace={current} />
      ) : (
        <EmptyState
          icon={SirenIcon}
          title="Only admins see alerts"
          description="Ask an owner or admin of this workspace."
        />
      )}
    </PageContainer>
  );
}
