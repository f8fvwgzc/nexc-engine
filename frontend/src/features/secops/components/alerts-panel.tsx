import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CheckIcon, SendIcon, SirenIcon } from 'lucide-react';
import { Link } from 'react-router-dom';
import { toast } from 'sonner';

import { ErrorState } from '@/components/custom-ui/error-state';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { acknowledgeAlert, alertsQuery } from '@/features/policies/api';
import { SeverityBadge } from '@/features/policies/components/severity-badge';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import { displayPrincipal } from '@/schemas/agent';
import type { Alert, AlertKind } from '@/schemas/policy';

import { orderAlerts } from '../alerts';
import { Panel, PanelEmpty } from './console-panel';

const KIND_LABEL: Record<AlertKind, string> = {
  dlp: 'Data leaving',
  risk: 'Behaviour',
  injection: 'Injection',
  policy: 'Policy',
};

/** One alert with its acknowledgement. */
export function AlertRow({
  alert,
  workspaceId,
  acknowledge,
}: {
  alert: Alert;
  workspaceId: string;
  acknowledge: boolean;
}) {
  const queryClient = useQueryClient();
  const seen = useMutation({
    mutationFn: () => acknowledgeAlert(alert.id),
    onSuccess: (next) => {
      queryClient.setQueriesData<Alert[]>({ queryKey: qk.alerts.all }, (list) =>
        list?.map((a) => (a.id === next.id ? next : a)),
      );
      void queryClient.invalidateQueries({ queryKey: qk.secops.overview(workspaceId) });
    },
    onError: (err) => toast.error(errorMessage(err)),
  });
  const run = alert.run_id
    ? `/app/runs/${alert.run_id}${alert.span_id ? `?span=${alert.span_id}` : ''}`
    : null;
  return (
    <li
      className={cn(
        'space-y-1 px-3 py-2 text-xs',
        alert.acknowledged_at && 'text-muted-foreground',
      )}
    >
      <div className="flex flex-wrap items-center gap-1.5">
        <SeverityBadge severity={alert.severity} />
        <Badge variant="outline" className="h-5 text-[10px]">
          {KIND_LABEL[alert.kind]}
        </Badge>
        {alert.deliveries > 0 && (
          <Badge
            variant="secondary"
            className="h-5 gap-1 text-[10px] font-normal"
            title="Delivered to the workspace's alert destinations"
          >
            <SendIcon className="size-3" aria-hidden />
            sent to {alert.deliveries} destination{alert.deliveries === 1 ? '' : 's'}
          </Badge>
        )}
        <span className="ml-auto">
          {alert.acknowledged_at ? (
            <span className="text-[11px]">seen</span>
          ) : acknowledge ? (
            <Button
              size="xs"
              variant="outline"
              disabled={seen.isPending}
              onClick={() => seen.mutate()}
            >
              <CheckIcon />
              Acknowledge
            </Button>
          ) : null}
        </span>
      </div>
      <p className="min-w-0">
        <span className="font-medium">{alert.title}</span>
        <span className="text-muted-foreground">
          {' '}
          · {displayPrincipal(alert.principal)} · <span className="font-mono">{alert.rule}</span>
          {run && (
            <>
              {' '}
              ·{' '}
              <Link to={run} className="inline-flex min-h-[1.6rem] items-center hover:underline">
                run {alert.run_id?.slice(0, 8)}
              </Link>
            </>
          )}{' '}
          · <time dateTime={alert.created_at}>{formatRelative(alert.created_at)}</time>
        </span>
      </p>
    </li>
  );
}

interface AlertsPanelProps {
  workspaceId: string;
  admin: boolean;
  className?: string;
}

/** What needs a look, unacknowledged first; acknowledged here. */
export function AlertsPanel({ workspaceId, admin, className }: AlertsPanelProps) {
  const { data, isPending, error, refetch } = useQuery({
    ...alertsQuery(workspaceId),
    enabled: admin,
  });
  const alerts = orderAlerts(data ?? []).slice(0, 40);
  const open = alerts.filter((a) => a.acknowledged_at === null).length;
  return (
    <Panel
      id="secops-alerts"
      title="Alerts"
      icon={SirenIcon}
      count={admin ? open : undefined}
      hint={admin && data && data.length > open ? `${data.length - open} acknowledged` : undefined}
      className={className}
      bodyClassName="p-0"
    >
      {!admin ? (
        <PanelEmpty>Admins see alerts.</PanelEmpty>
      ) : isPending ? (
        <div className="p-3">
          <Skeleton className="h-16 rounded-lg" />
        </div>
      ) : error ? (
        <ErrorState
          compact
          className="m-3"
          title="Couldn’t load the alerts"
          error={error}
          onRetry={() => void refetch()}
        />
      ) : alerts.length === 0 ? (
        <PanelEmpty icon={SirenIcon}>
          Nothing to look at: no data leaving, no odd behaviour.
        </PanelEmpty>
      ) : (
        <ul className="max-h-72 divide-y overflow-y-auto" aria-label="Alerts">
          {alerts.map((alert) => (
            <AlertRow key={alert.id} alert={alert} workspaceId={workspaceId} acknowledge />
          ))}
        </ul>
      )}
    </Panel>
  );
}
