import { useQueries, useSuspenseQuery } from '@tanstack/react-query';
import { PlayIcon } from 'lucide-react';
import { Suspense } from 'react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton, TableSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { graphsQuery } from '@/features/graphs/api';
import { graphRunsQuery } from '@/features/runs/api';
import { RunsTable, type RunRow } from '@/features/runs/components/runs-table';

const MAX_ROWS = 100;

function RecentRuns() {
  const { data: graphs } = useSuspenseQuery(graphsQuery());
  const results = useQueries({ queries: graphs.map((g) => graphRunsQuery(g.id)) });
  const loading = results.some((r) => r.isPending);
  const rows: RunRow[] = results
    .flatMap((r, i) => (r.data ?? []).map((run) => ({ run, graphName: graphs[i]?.name ?? '—' })))
    .sort((a, b) => b.run.created_at.localeCompare(a.run.created_at))
    .slice(0, MAX_ROWS);

  return (
    <div className="mx-auto w-full max-w-6xl space-y-6 p-4 sm:p-6">
      <PageHeader title="Runs" description="Recent executions across all of your graphs." />
      {loading && rows.length === 0 ? (
        <TableSkeleton />
      ) : rows.length === 0 ? (
        <EmptyState
          icon={PlayIcon}
          title="No runs yet"
          description="Open a graph and press Run to execute it. Runs show up here with tokens, cost and artifacts."
          action={
            <Button asChild>
              <Link to="/app">Go to graphs</Link>
            </Button>
          }
        />
      ) : (
        <RunsTable rows={rows} />
      )}
    </div>
  );
}

export default function RunsPage() {
  return (
    <>
      <Seo title="Runs" noIndex />
      <Suspense fallback={<PageSkeleton />}>
        <RecentRuns />
      </Suspense>
    </>
  );
}
