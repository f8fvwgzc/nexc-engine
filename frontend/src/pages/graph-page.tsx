import { Suspense } from 'react';
import { useParams } from 'react-router-dom';

import { QueryErrorBoundary } from '@/components/layout/query-error-boundary';
import { Seo } from '@/components/seo/seo';
import { CanvasSkeleton } from '@/features/graph/components/canvas-skeleton';
import { GraphWorkspace } from '@/features/graph/components/graph-workspace';

export default function GraphPage() {
  const { graphId = '' } = useParams();
  return (
    // A graph that failed to load says so in the page; one that is gone is the route's to say.
    <QueryErrorBoundary key={graphId} page="default" title="Couldn’t load the graph">
      <Suspense
        fallback={
          <>
            <Seo title="Loading graph…" noIndex />
            <CanvasSkeleton />
          </>
        }
      >
        <GraphWorkspace graphId={graphId} />
      </Suspense>
    </QueryErrorBoundary>
  );
}
