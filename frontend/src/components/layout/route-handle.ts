/** Static breadcrumb or a dynamic one resolved from cached server data. */
export type Crumb = { label: string; to?: string } | { dynamic: 'graph' | 'run' };

export interface RouteHandle {
  crumbs: Crumb[];
}

export function isRouteHandle(value: unknown): value is RouteHandle {
  return (
    typeof value === 'object' && value !== null && Array.isArray((value as RouteHandle).crumbs)
  );
}
