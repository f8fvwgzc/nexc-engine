import type { SearchHit } from './api';

/** Where a thing the search found lives in the app. */
export function pageOf(hit: Pick<SearchHit, 'kind' | 'id'>): string {
  switch (hit.kind) {
    case 'issue':
      return `/app/issues?issue=${hit.id}`;
    case 'graph':
      return `/app/graphs/${hit.id}`;
    case 'project':
      return `/app/projects/${hit.id}`;
    case 'team':
      return `/app/issues?team=${hit.id}`;
    case 'member':
      return '/app/settings/members';
    case 'document':
      return '/app/settings/knowledge';
  }
}
