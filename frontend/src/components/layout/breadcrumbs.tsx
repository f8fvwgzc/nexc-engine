import { useQuery } from '@tanstack/react-query';
import type { ReactNode } from 'react';
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
import { issuePath } from '@/features/issues/links';
import { teamsQuery } from '@/features/workspaces/api';
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

/** One crumb, linked unless it is where the reader is. */
interface CrumbItem {
  key: string;
  label: ReactNode;
  to?: string;
  title?: string;
}

/**
 * The crumbs of an issue's page, after "Issues": its team, its parent if it has one, then the
 * issue itself (`ENG-7 Pick the bus`, cut short when long). Read from the caches the issue page
 * fills (the teams of the issue's own workspace, which need not be the current one); a team not
 * read yet is just "Team", so nothing is left loading once the issue is there.
 */
function IssueCrumbs({ first, last }: { first: boolean; last: boolean }) {
  const { issueId = '' } = useParams();
  const { data: issue, status } = useQuery({ ...issueQuery(issueId), enabled: false });
  const { data: teams } = useQuery({
    ...teamsQuery(issue?.workspace_id ?? ''),
    enabled: false,
  });
  const items: CrumbItem[] = [];
  if (issue) {
    items.push({
      key: 'team',
      label: teams?.find((t) => t.id === issue.team_id)?.name ?? 'Team',
      to: `/app/issues?team=${issue.team_id}`,
    });
    if (issue.parent) {
      items.push({
        key: 'parent',
        label: issue.parent.identifier,
        title: `${issue.parent.identifier} ${issue.parent.title}`,
        to: issuePath(issue.parent.id),
      });
    }
    const name = `${issue.identifier} ${issue.title}`;
    items.push({ key: 'issue', label: name, title: name });
  } else {
    items.push({
      key: 'issue',
      label:
        status === 'error' ? (
          'Issue'
        ) : (
          <Skeleton className="h-4 w-24">
            <span className="sr-only">Loading the issue</span>
          </Skeleton>
        ),
    });
  }
  return items.map((item, i) => (
    <CrumbEntry
      key={item.key}
      item={item}
      first={first && i === 0}
      last={last && i === items.length - 1}
    />
  ));
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
  return <>Run {runId.slice(0, 8)}</>;
}

function CrumbLabel({ crumb }: { crumb: Crumb }) {
  if ('label' in crumb) return <>{crumb.label}</>;
  if (crumb.dynamic === 'graph') return <GraphCrumb />;
  if (crumb.dynamic === 'project') return <ProjectCrumb />;
  if (crumb.dynamic === 'artifact') return <ArtifactCrumb />;
  return <RunCrumb />;
}

/**
 * One crumb and the separator before it. Every crumb but the last is hidden on a phone, and
 * only the last one shrinks (cut short) when the row is too narrow; all share one typeface.
 */
function CrumbEntry({ item, first, last }: { item: CrumbItem; first: boolean; last: boolean }) {
  return (
    <>
      {!first && <BreadcrumbSeparator className="hidden sm:block" />}
      <BreadcrumbItem className={last ? 'min-w-0' : 'hidden sm:inline-flex'}>
        {!last && item.to ? (
          <BreadcrumbLink asChild>
            <Link to={item.to} title={item.title} className="max-w-40 truncate">
              {item.label}
            </Link>
          </BreadcrumbLink>
        ) : (
          <BreadcrumbPage className="truncate" title={item.title}>
            {item.label}
          </BreadcrumbPage>
        )}
      </BreadcrumbItem>
    </>
  );
}

export function Breadcrumbs() {
  const crumbs = useMatches().flatMap((m) => (isRouteHandle(m.handle) ? m.handle.crumbs : []));
  if (crumbs.length === 0) return null;

  return (
    <Breadcrumb className="min-w-0">
      <BreadcrumbList className="flex-nowrap">
        {crumbs.map((crumb, i) => {
          const first = i === 0;
          const last = i === crumbs.length - 1;
          // An issue's crumb stands for several: its team, its parent and the issue.
          if ('dynamic' in crumb && crumb.dynamic === 'issue') {
            return <IssueCrumbs key={i} first={first} last={last} />;
          }
          return (
            <CrumbEntry
              key={i}
              item={{
                key: String(i),
                label: <CrumbLabel crumb={crumb} />,
                to: 'to' in crumb ? crumb.to : undefined,
              }}
              first={first}
              last={last}
            />
          );
        })}
      </BreadcrumbList>
    </Breadcrumb>
  );
}
