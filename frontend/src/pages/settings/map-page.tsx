import { useQuery } from '@tanstack/react-query';
import { ArrowRightIcon, UsersIcon, WaypointsIcon } from 'lucide-react';
import { useState } from 'react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { ContentSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { workspaceMapQuery } from '@/features/insight/api';
import { MapExplorer } from '@/features/insight/map-explorer';
import { followable } from '@/features/insight/map-kinds';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { formatInteger } from '@/lib/format';
import type { MapKind } from '@/schemas/insight';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

function WorkspaceMap({ workspace }: { workspace: Workspace }) {
  const { data: map, isPending, error, refetch } = useQuery(workspaceMapQuery(workspace.id));
  const [kind, setKind] = useState<MapKind>('member');
  if (isPending) return <Skeleton className="h-72 w-full rounded-lg" />;
  if (error) {
    return (
      <ErrorState title="Couldn’t load the map" error={error} onRetry={() => void refetch()} />
    );
  }
  // A workspace with nothing in it yet has nothing to map: say what fills it.
  if (map.entities.every((e) => e.count === 0)) {
    return (
      <EmptyState
        icon={WaypointsIcon}
        title="Nothing to map yet"
        description="Members, teams, projects, issues, graphs, documents and agents appear here, with the ties between them, as the workspace fills."
        action={
          <Button asChild>
            <Link to="/app/settings/members">
              <UsersIcon />
              Invite members
            </Link>
          </Button>
        }
      />
    );
  }
  const label = new Map(map.entities.map((e) => [e.key, e.label]));
  return (
    <div className="space-y-6">
      <section aria-label="What the workspace holds" className="space-y-2">
        <h2 className="text-[13px] font-medium">What the workspace holds</h2>
        <ul className="grid grid-cols-2 gap-2 sm:grid-cols-4">
          {map.entities.map((entity) => {
            const opens = followable(entity.key);
            const tile = (
              <>
                <p className="text-lg font-semibold tabular-nums">{formatInteger(entity.count)}</p>
                <p className="text-xs text-muted-foreground">{entity.label}</p>
              </>
            );
            return (
              <li key={entity.key}>
                {opens ? (
                  <button
                    type="button"
                    aria-pressed={opens === kind}
                    title={`Follow the ties of ${entity.label.toLowerCase()}`}
                    className="w-full rounded-lg border px-3 py-2 text-left hover:bg-muted/50 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-pressed:border-primary"
                    onClick={() => {
                      setKind(opens);
                      document
                        .getElementById('follow-the-ties')
                        ?.scrollIntoView({ block: 'nearest' });
                    }}
                  >
                    {tile}
                  </button>
                ) : (
                  <div className="rounded-lg border px-3 py-2">{tile}</div>
                )}
              </li>
            );
          })}
        </ul>
      </section>
      <section aria-label="How they relate" className="space-y-2">
        <h2 className="text-[13px] font-medium">How they relate</h2>
        <ul className="divide-y rounded-lg border">
          {map.relations.map((relation) => (
            <li
              key={`${relation.from}:${relation.label}:${relation.to}`}
              className="flex items-center gap-2 px-3 py-2 text-[13px]"
            >
              <span className="w-24 font-medium">{label.get(relation.from) ?? relation.from}</span>
              <span className="flex min-w-0 flex-1 items-center gap-1.5 text-muted-foreground">
                {relation.label}
                <ArrowRightIcon className="size-3.5" aria-hidden />
              </span>
              <span className="w-24 font-medium">{label.get(relation.to) ?? relation.to}</span>
              <span className="w-16 text-right text-xs text-muted-foreground tabular-nums">
                {formatInteger(relation.count)}
              </span>
            </li>
          ))}
        </ul>
      </section>
      <div id="follow-the-ties">
        <MapExplorer workspaceId={workspace.id} kind={kind} onKindChange={setKind} />
      </div>
    </div>
  );
}

export default function MapPage() {
  const { current } = useCurrentWorkspace();
  return (
    <PageContainer width="narrow">
      <Seo title="Workspace map" noIndex />
      <PageHeader
        title="Workspace map"
        description="The kinds of things in this workspace and the ties between them, with today's numbers. Pick any one member, team, project, issue, graph, document or agent to see exactly what it is tied to, and follow on from there."
      />
      {!current ? (
        <ContentSkeleton />
      ) : isWorkspaceAdmin(current.role) ? (
        <WorkspaceMap key={current.id} workspace={current} />
      ) : (
        <EmptyState
          icon={WaypointsIcon}
          title="Only admins see the workspace map"
          description="Its counts span every team. Ask an owner or admin."
        />
      )}
    </PageContainer>
  );
}
