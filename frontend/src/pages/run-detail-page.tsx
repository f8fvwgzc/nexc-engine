import { useQuery, useSuspenseQuery } from '@tanstack/react-query';
import {
  ClockIcon,
  CoinsIcon,
  CpuIcon,
  ListChecksIcon,
  SquareIcon,
  UserCheckIcon,
} from 'lucide-react';
import { Suspense } from 'react';
import { Link, useParams, useSearchParams } from 'react-router-dom';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { StatTile } from '@/components/custom-ui/stat-tile';
import { StatusBadge } from '@/components/custom-ui/status-badge';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { graphQuery } from '@/features/graphs/api';
import { runQuery, runTraceQuery } from '@/features/runs/api';
import { ArtifactsList } from '@/features/runs/components/artifacts-list';
import { EvidencePanel } from '@/features/runs/components/evidence';
import { NodeRunsTable } from '@/features/runs/components/node-runs-table';
import { TraceVerification } from '@/features/runs/components/trace-verification';
import { TraceWaterfall } from '@/features/runs/components/trace-waterfall';
import { useCancelRun } from '@/features/runs/hooks/use-cancel-run';
import { useLiveTrace } from '@/features/runs/hooks/use-live-trace';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import {
  durationBetween,
  formatCost,
  formatDateTime,
  formatDuration,
  formatTokens,
} from '@/lib/format';
import { isRunActive } from '@/schemas/run';
import { isWorkspaceAdmin } from '@/schemas/workspace';

function RunDetail({ runId }: { runId: string }) {
  const [params] = useSearchParams();
  // `?span=` opens the waterfall at one span: the console's feed and explorer link here.
  const highlight = params.get('span');
  const { data: run } = useSuspenseQuery(runQuery(runId));
  const { data: graph } = useQuery(graphQuery(run.graph_id));
  const { current } = useCurrentWorkspace();
  const admin = current !== undefined && isWorkspaceAdmin(current.role);
  const waiting = run.node_runs.filter((nr) => nr.status === 'waiting_approval');
  const cancelRun = useCancelRun();
  const titles = new Map(graph?.nodes.map((n) => [n.id, n.title]) ?? []);
  const duration = durationBetween(run.started_at, run.finished_at);
  const done = run.node_runs.filter(
    (nr) => !['queued', 'running', 'idle'].includes(nr.status),
  ).length;
  const active = isRunActive(run.status);
  const trace = useQuery(runTraceQuery(run.id));
  useLiveTrace(run);

  return (
    <div className="mx-auto w-full max-w-6xl space-y-8 p-4 sm:p-6">
      <PageHeader
        title={
          <span className="flex flex-wrap items-center gap-3">
            <span className="font-mono">Run {run.id.slice(0, 8)}</span>
            <StatusBadge status={run.status} />
          </span>
        }
        description={
          <>
            {graph ? (
              <Link
                to={`/app/graphs/${graph.id}`}
                className="font-medium text-foreground hover:underline"
              >
                {graph.name}
              </Link>
            ) : (
              'Graph'
            )}{' '}
            · created {formatDateTime(run.created_at)}
          </>
        }
        actions={
          <>
            {active && (
              <AnimatedButton
                variant="destructive"
                loading={cancelRun.isPending}
                onClick={() => cancelRun.mutate(run.id)}
              >
                <SquareIcon className="fill-current" />
                Cancel run
              </AnimatedButton>
            )}
            <Button variant="outline" asChild>
              <Link to={`/app/graphs/${run.graph_id}`}>Open canvas</Link>
            </Button>
          </>
        }
      />
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <StatTile label="Nodes" value={`${done}/${run.node_runs.length}`} icon={ListChecksIcon} />
        <StatTile
          label="Duration"
          value={duration === null ? '—' : formatDuration(duration)}
          icon={ClockIcon}
          hint={run.started_at ? `started ${formatDateTime(run.started_at)}` : 'not started'}
        />
        <StatTile
          label="Tokens"
          value={formatTokens(run.tokens_in + run.tokens_out)}
          icon={CpuIcon}
          hint={`${formatTokens(run.tokens_in)} in · ${formatTokens(run.tokens_out)} out`}
        />
        <StatTile label="Cost" value={formatCost(run.cost_usd)} icon={CoinsIcon} />
      </div>
      {waiting.length > 0 && (
        <p
          role="status"
          className="flex items-center gap-2 rounded-lg border border-[color-mix(in_oklch,var(--status-queued)_40%,transparent)] bg-[color-mix(in_oklch,var(--status-queued)_10%,transparent)] px-3 py-2 text-sm"
        >
          <UserCheckIcon className="size-4 shrink-0" aria-hidden />
          <span>
            {waiting.length === 1
              ? `${titles.get(waiting[0]?.node_id ?? '') ?? 'A node'} waits for an approval`
              : `${waiting.length} nodes wait for an approval`}
            : a tool call is held until an admin decides it from the{' '}
            <Link to="/app/inbox" className="font-medium underline">
              Inbox
            </Link>
            . The run goes on meanwhile.
          </span>
        </p>
      )}
      <section className="space-y-3" aria-labelledby="node-runs">
        <h2 id="node-runs" className="text-sm font-medium">
          Node runs
        </h2>
        <NodeRunsTable graphId={run.graph_id} nodeRuns={run.node_runs} titles={titles} />
      </section>
      {run.node_runs.some((nr) => nr.evidence) && (
        <section className="space-y-3" aria-labelledby="evidence">
          <h2 id="evidence" className="text-sm font-medium">
            Evidence
            <span className="ml-2 font-normal text-muted-foreground">
              what each output cites of what its node was given
            </span>
          </h2>
          <ul className="divide-y rounded-xl border">
            {run.node_runs
              .filter((nr) => nr.evidence)
              .map((nr) => (
                <li key={nr.node_id} className="space-y-2 px-3 py-2">
                  <p className="text-sm font-medium">
                    {titles.get(nr.node_id) ?? nr.node_id.slice(0, 8)}
                  </p>
                  {nr.evidence && (
                    <EvidencePanel
                      evidence={nr.evidence}
                      graphId={run.graph_id}
                      runId={run.id}
                      titles={titles}
                    />
                  )}
                </li>
              ))}
          </ul>
        </section>
      )}
      <section className="space-y-3" aria-labelledby="trace">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 id="trace" className="flex items-center gap-2 text-sm font-medium">
            Trace
            {run.policy_mismatches > 0 && (
              <Badge
                variant="destructive"
                title="Actions the runtime reported that the run's policy did not allow; each is a policy span and an audit entry"
              >
                {run.policy_mismatches} policy mismatch{run.policy_mismatches === 1 ? '' : 'es'}
              </Badge>
            )}
          </h2>
          <TraceVerification runId={run.id} active={active} />
        </div>
        {trace.data ? (
          <TraceWaterfall spans={trace.data} titles={titles} live={active} highlight={highlight} />
        ) : (
          <Skeleton className="h-24 rounded-xl" />
        )}
      </section>
      <section className="space-y-3" aria-labelledby="artifacts">
        <h2 id="artifacts" className="text-sm font-medium">
          Artifacts
        </h2>
        <ArtifactsList runId={run.id} titles={titles} admin={admin} />
      </section>
    </div>
  );
}

export default function RunDetailPage() {
  const { runId = '' } = useParams();
  return (
    <>
      <Seo title={`Run ${runId.slice(0, 8)}`} noIndex />
      <Suspense fallback={<PageSkeleton />}>
        <RunDetail runId={runId} />
      </Suspense>
    </>
  );
}
