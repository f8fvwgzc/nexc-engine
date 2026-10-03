import { Suspense } from 'react';
import { useParams } from 'react-router-dom';

import { Seo } from '@/components/seo/seo';
import { CanvasSkeleton } from '@/features/graph/components/canvas-skeleton';
import { GraphWorkspace } from '@/features/graph/components/graph-workspace';

export default function GraphPage() {
  const { graphId = '' } = useParams();
  return (
    <Suspense
      fallback={
        <>
          <Seo title="Loading graph…" noIndex />
          <CanvasSkeleton />
        </>
      }
    >
      <GraphWorkspace key={graphId} graphId={graphId} />
    </Suspense>
  );
}
