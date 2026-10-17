import { useQuery, useSuspenseQuery } from '@tanstack/react-query';
import { ClockIcon, CoinsIcon, CpuIcon, ListChecksIcon, SquareIcon } from 'lucide-react';
import { Suspense } from 'react';
import { Link, useParams } from 'react-router-dom';

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
import { NodeRunsTable } from '@/features/runs/components/node-runs-table';
import { TraceVerification } from '@/features/runs/components/trace-verification';
import { TraceWaterfall } from '@/features/runs/components/trace-waterfall';
import { useCancelRun } from '@/features/runs/hooks/use-cancel-run';
import { useLiveTrace } from '@/features/runs/hooks/use-live-trace';
import {
  durationBetween,
  formatCost,
  formatDateTime,
  formatDuration,
  formatTokens,
} from '@/lib/format';
import { isRunActive } from '@/schemas/run';

function RunDetail({ runId }: { runId: string }) {
  const { data: run } = useSuspenseQuery(runQuery(runId));
  const { data: graph } = useQuery(graphQuery(run.graph_id));
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
      <section className="space-y-3" aria-labelledby="node-runs">
        <h2 id="node-runs" className="text-sm font-medium">
          Node runs
        </h2>
        <NodeRunsTable graphId={run.graph_id} nodeRuns={run.node_runs} titles={titles} />
      </section>
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
          <TraceWaterfall spans={trace.data} titles={titles} live={active} />
        ) : (
          <Skeleton className="h-24 rounded-xl" />
        )}
      </section>
      <section className="space-y-3" aria-labelledby="artifacts">
        <h2 id="artifacts" className="text-sm font-medium">
          Artifacts
        </h2>
        <ArtifactsList runId={run.id} titles={titles} />
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
