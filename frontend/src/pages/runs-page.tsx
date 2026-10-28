import { useSuspenseQuery } from '@tanstack/react-query';
import { PlayIcon } from 'lucide-react';
import { Suspense } from 'react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { FadeIn } from '@/components/custom-ui/motion';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { TableSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { workspaceRunsQuery } from '@/features/runs/api';
import { RunsTable, type RunRow } from '@/features/runs/components/runs-table';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';

const TITLE = 'Runs';
const DESCRIPTION = 'Recent executions across all of your graphs.';

/** The page while its runs load: the same header, then the table's shape. */
function RunsSkeleton() {
  return (
    <PageContainer role="status" aria-busy aria-label="Loading">
      <div className="space-y-2 border-b pb-4">
        <Skeleton className="h-7 w-24" />
        <Skeleton className="h-4 w-72 max-w-full" />
      </div>
      <TableSkeleton rows={8} />
    </PageContainer>
  );
}

function RecentRuns({ workspaceId }: { workspaceId: string }) {
  // One request for the newest runs of every graph the caller may open, with their graphs' names.
  const { data: runs } = useSuspenseQuery(workspaceRunsQuery(workspaceId));
  const rows: RunRow[] = runs.map((run) => ({ run, graphName: run.graph_name }));

  return (
    <PageContainer>
      <PageHeader title={TITLE} description={DESCRIPTION} />
      {rows.length === 0 ? (
        <FadeIn>
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
        </FadeIn>
      ) : (
        <FadeIn variant="fade">
          <RunsTable rows={rows} />
        </FadeIn>
      )}
    </PageContainer>
  );
}

export default function RunsPage() {
  const workspaceId = useWorkspaceId();
  return (
    <>
      <Seo title={TITLE} noIndex />
      {workspaceId ? (
        <Suspense fallback={<RunsSkeleton />}>
          <RecentRuns workspaceId={workspaceId} />
        </Suspense>
      ) : (
        <RunsSkeleton />
      )}
    </>
  );
}
