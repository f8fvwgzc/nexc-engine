import { useQuery } from '@tanstack/react-query';
import { WaypointsIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { Button } from '@/components/ui/button';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';
import { entityPath } from '@/schemas/ontology';

import { ontologyStatusQuery } from './api';

/**
 * "On the map": opens the thing as onto-intelli models it (its links, history and impact) in the
 * embedded UI. Shown only when onto-intelli is configured and the reader may open it.
 */
export function OntologyLink({
  entity,
  compact = false,
  size = 'sm',
}: {
  entity: string;
  compact?: boolean;
  /** `default` beside a page header's buttons. */
  size?: 'sm' | 'default';
}) {
  const workspaceId = useWorkspaceId();
  const { data } = useQuery({
    ...ontologyStatusQuery(workspaceId ?? ''),
    enabled: Boolean(workspaceId),
    retry: false,
  });
  if (!data || data.mode === 'off') return null;
  return (
    <Button variant="outline" size={size} asChild>
      <Link to={entityPath(entity)} title="Open it on the workspace map (onto-intelli)">
        <WaypointsIcon />
        {compact ? <span className="sr-only">On the map</span> : 'On the map'}
      </Link>
    </Button>
  );
}
