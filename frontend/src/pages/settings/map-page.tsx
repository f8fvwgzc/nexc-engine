import { useQuery } from '@tanstack/react-query';
import { BookOpenIcon, WaypointsIcon } from 'lucide-react';
import { useSearchParams } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { ContentSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { ontologyStatusQuery } from '@/features/ontology/api';
import { OntoFrame } from '@/features/ontology/onto-frame';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { ontoView } from '@/schemas/ontology';
import type { Workspace } from '@/schemas/workspace';

const DOCS = 'https://f8fvwgzc.github.io/nexc-engine/operate/ontology';

/** The view of onto-intelli's UI to open: `?view=/entities/<id>`, else the map (`?focus=<id>`). */
function viewOf(params: URLSearchParams, workspace: string): string {
  const asked = params.get('view');
  if (asked) return ontoView(asked);
  const map = new URLSearchParams({ workspace });
  const focus = params.get('focus');
  if (focus) map.set('focus', focus);
  return `/map?${map.toString()}`;
}

function OntologyMap({ workspace }: { workspace: Workspace }) {
  const [params] = useSearchParams();
  const { data: status, isPending, error, refetch } = useQuery(ontologyStatusQuery(workspace.id));
  if (isPending) return <Skeleton className="h-[70vh] w-full rounded-lg" />;
  if (error) {
    return (
      <ErrorState title="Couldn’t load the map" error={error} onRetry={() => void refetch()} />
    );
  }
  if (status.mode === 'off' || !status.workspace) {
    return (
      <EmptyState
        icon={WaypointsIcon}
        title="onto-intelli is not configured"
        description="The workspace map is onto-intelli’s: its teams, people, projects, cycles, issues, agents, graphs, documents and runs, and the links between them. This server runs without it (NEXC_ONTO_MODE=off), so graphs use the read-only starter vocabulary. An operator sets NEXC_ONTO_URL to turn the map on."
        action={
          <Button variant="outline" asChild>
            <a href={DOCS} target="_blank" rel="noreferrer">
              <BookOpenIcon />
              How to set it up
            </a>
          </Button>
        }
      />
    );
  }
  return (
    <OntoFrame
      workspaceId={workspace.id}
      view={viewOf(params, status.workspace)}
      title={`Workspace map of ${workspace.name}`}
    />
  );
}

export default function MapPage() {
  const { current } = useCurrentWorkspace();
  return (
    <PageContainer width="narrow">
      <Seo title="Workspace map" noIndex />
      <PageHeader
        title="Workspace map"
        description="The workspace as onto-intelli models it: teams, people, projects, cycles, issues, agents, graphs, documents and runs, and how they relate. Explore paths and impact, and configure entity kinds, link types and actions there."
      />
      {!current ? (
        <ContentSkeleton />
      ) : current.role === 'guest' ? (
        <EmptyState
          icon={WaypointsIcon}
          title="Guests do not see the workspace map"
          description="It spans the whole workspace. Ask an owner or admin for a member’s role."
        />
      ) : (
        <OntologyMap key={current.id} workspace={current} />
      )}
    </PageContainer>
  );
}
