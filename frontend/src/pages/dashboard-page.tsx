import { useSuspenseQuery } from '@tanstack/react-query';
import { Suspense } from 'react';

import { PageHeader } from '@/components/custom-ui/page-header';
import { Stagger } from '@/components/custom-ui/motion';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { graphsQuery } from '@/features/graphs/api';
import { CreateGraphDialog } from '@/features/graphs/components/create-graph-dialog';
import { DashboardStats } from '@/features/graphs/components/dashboard-stats';
import { FirstRunHero } from '@/features/graphs/components/first-run-hero';
import { GraphCard } from '@/features/graphs/components/graph-card';
import { TemplateGallery } from '@/features/graphs/components/template-gallery';

function Dashboard() {
  const { data: graphs } = useSuspenseQuery(graphsQuery());
  const sorted = [...graphs].sort((a, b) => b.updated_at.localeCompare(a.updated_at));

  return (
    <div className="mx-auto w-full max-w-6xl space-y-8 p-4 sm:p-6">
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
          <section aria-label="Your graphs">
            <Stagger className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {sorted.map((graph) => (
                <GraphCard key={graph.id} graph={graph} />
              ))}
            </Stagger>
          </section>
          <TemplateGallery />
        </>
      )}
    </div>
  );
}

export default function DashboardPage() {
  return (
    <>
      <Seo title="Graphs" noIndex />
      <Suspense fallback={<PageSkeleton />}>
        <Dashboard />
      </Suspense>
    </>
  );
}
