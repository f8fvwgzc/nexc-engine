import { useQuery } from '@tanstack/react-query';
import { Fragment } from 'react';
import { Link, useMatches, useParams } from 'react-router-dom';

import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from '@/components/ui/breadcrumb';
import { Skeleton } from '@/components/ui/skeleton';
import { artifactFileQuery } from '@/features/artifacts/api';
import { graphQuery } from '@/features/graphs/api';
import { issueQuery, projectsQuery } from '@/features/issues/api';
import { useWorkspaceStore } from '@/stores/workspace-store';

import { isRouteHandle, type Crumb } from './route-handle';

function GraphCrumb() {
  const { graphId = '' } = useParams();
  // Reads the cache the graph page fills; never triggers its own fetch.
  const { data: name, status } = useQuery({
    ...graphQuery(graphId),
    enabled: false,
    select: (g) => g.name,
  });
  if (name) return <>{name}</>;
  // The page says why the graph cannot be shown; the crumb only names what was asked for.
  if (status === 'error') return <>Graph</>;
  return (
    <Skeleton className="h-4 w-24">
      <span className="sr-only">Loading the graph’s name</span>
    </Skeleton>
  );
}

function IssueCrumb() {
  const { issueId = '' } = useParams();
  // Reads the cache the issue page fills; never triggers its own fetch.
  const { data: identifier, status } = useQuery({
    ...issueQuery(issueId),
    enabled: false,
    select: (i) => i.identifier,
  });
  if (identifier) return <span className="font-mono">{identifier}</span>;
  if (status === 'error') return <>Issue</>;
  return (
    <Skeleton className="h-4 w-16">
      <span className="sr-only">Loading the issue’s identifier</span>
    </Skeleton>
  );
}

function ProjectCrumb() {
  const { projectId = '' } = useParams();
  const workspaceId = useWorkspaceStore((s) => s.currentId) ?? '';
  // Reads the list the project page fills; never triggers its own fetch.
  const { data: name, status } = useQuery({
    ...projectsQuery(workspaceId),
    enabled: false,
    select: (projects) => projects.find((p) => p.id === projectId)?.name,
  });
  if (name) return <>{name}</>;
  if (status !== 'pending') return <>Project</>;
  return (
    <Skeleton className="h-4 w-24">
      <span className="sr-only">Loading the project’s name</span>
    </Skeleton>
  );
}

function ArtifactCrumb() {
  const { fileId = '' } = useParams();
  // Reads the cache the file page fills; never triggers its own fetch.
  const { data: name, status } = useQuery({
    ...artifactFileQuery(fileId),
    enabled: false,
    select: (f) => f.name,
  });
  if (name) return <>{name}</>;
  if (status === 'error') return <>File</>;
  return (
    <Skeleton className="h-4 w-24">
      <span className="sr-only">Loading the file’s name</span>
    </Skeleton>
  );
}

function RunCrumb() {
  const { runId = '' } = useParams();
  return <span className="font-mono">Run {runId.slice(0, 8)}</span>;
}

function CrumbLabel({ crumb }: { crumb: Crumb }) {
  if ('label' in crumb) return <>{crumb.label}</>;
  if (crumb.dynamic === 'graph') return <GraphCrumb />;
  if (crumb.dynamic === 'issue') return <IssueCrumb />;
  if (crumb.dynamic === 'project') return <ProjectCrumb />;
  if (crumb.dynamic === 'artifact') return <ArtifactCrumb />;
  return <RunCrumb />;
}

export function Breadcrumbs() {
  const crumbs = useMatches().flatMap((m) => (isRouteHandle(m.handle) ? m.handle.crumbs : []));
  if (crumbs.length === 0) return null;

  return (
    <Breadcrumb className="min-w-0">
      <BreadcrumbList className="flex-nowrap">
        {crumbs.map((crumb, i) => {
          const last = i === crumbs.length - 1;
          return (
            <Fragment key={i}>
              {i > 0 && <BreadcrumbSeparator className="hidden sm:block" />}
              <BreadcrumbItem className={last ? 'min-w-0' : 'hidden sm:inline-flex'}>
                {!last && 'to' in crumb && crumb.to ? (
                  <BreadcrumbLink asChild>
                    <Link to={crumb.to}>
                      <CrumbLabel crumb={crumb} />
                    </Link>
                  </BreadcrumbLink>
                ) : (
                  <BreadcrumbPage className="truncate">
                    <CrumbLabel crumb={crumb} />
                  </BreadcrumbPage>
                )}
              </BreadcrumbItem>
            </Fragment>
          );
        })}
      </BreadcrumbList>
    </Breadcrumb>
  );
}
