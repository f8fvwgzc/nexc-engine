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
import { graphQuery } from '@/features/graphs/api';

import { isRouteHandle, type Crumb } from './route-handle';

function GraphCrumb() {
  const { graphId = '' } = useParams();
  // Reads the cache the graph page fills; never triggers its own fetch.
  const { data: name } = useQuery({
    ...graphQuery(graphId),
    enabled: false,
    select: (g) => g.name,
  });
  return name ? <>{name}</> : <Skeleton className="h-4 w-24" />;
}

function RunCrumb() {
  const { runId = '' } = useParams();
  return <span className="font-mono">Run {runId.slice(0, 8)}</span>;
}

function CrumbLabel({ crumb }: { crumb: Crumb }) {
  if ('label' in crumb) return <>{crumb.label}</>;
  return crumb.dynamic === 'graph' ? <GraphCrumb /> : <RunCrumb />;
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
