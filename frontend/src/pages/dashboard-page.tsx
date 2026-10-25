import { useSuspenseQuery } from '@tanstack/react-query';
import { Suspense } from 'react';

import { Stagger } from '@/components/custom-ui/motion';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { QueryErrorBoundary } from '@/components/layout/query-error-boundary';
import { Seo } from '@/components/seo/seo';
import { graphsQuery } from '@/features/graphs/api';
import { CreateGraphDialog } from '@/features/graphs/components/create-graph-dialog';
import { DashboardStats } from '@/features/graphs/components/dashboard-stats';
import { FirstRunHero } from '@/features/graphs/components/first-run-hero';
import { GraphCard } from '@/features/graphs/components/graph-card';
import { TemplateGallery } from '@/features/graphs/components/template-gallery';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';

function Dashboard() {
  const workspaceId = useWorkspaceId();
  const { data: graphs } = useSuspenseQuery(graphsQuery(workspaceId));
  const sorted = [...graphs].sort((a, b) => b.updated_at.localeCompare(a.updated_at));

  return (
    // The page fades in as it replaces its placeholder; its cards follow one after another.
    <PageContainer className="motion-safe:animate-fade-in">
      {graphs.length === 0 ? (
        <>
          <FirstRunHero />
          <CreateGraphDialog showTrigger={false} />
        </>
      ) : (
        <>
          <PageHeader
            title="Graphs"
            description="Your knowledge graphs. Open one to edit, plan and run it."
            actions={<CreateGraphDialog />}
          />
          <DashboardStats graphs={graphs} />
          <section aria-labelledby="your-graphs">
            <h2 id="your-graphs" className="sr-only">
              Your graphs
            </h2>
            {/* One column on a phone: `grid-cols-1` lets a card shrink below its content's width. */}
            <Stagger className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {sorted.map((graph) => (
                <GraphCard key={graph.id} graph={graph} />
              ))}
            </Stagger>
          </section>
          <TemplateGallery />
        </>
      )}
    </PageContainer>
  );
}

export default function DashboardPage() {
  const workspaceId = useWorkspaceId();
  return (
    <>
      <Seo title="Graphs" noIndex />
      <QueryErrorBoundary key={workspaceId} page="default" title="Couldn’t load the graphs">
        <Suspense fallback={<PageSkeleton />}>
          <Dashboard />
        </Suspense>
      </QueryErrorBoundary>
    </>
  );
}
