import { useQuery } from '@tanstack/react-query';
import { useEffect } from 'react';

import { graphRunsQuery } from '@/features/runs/api';
import { useGraphStore } from '@/stores/graph-store';

/** Seeds the live run state from the newest run, so a reload mid-run keeps showing progress. */
export function useHydrateLatestRun(graphId: string) {
  const { data: latest } = useQuery({ ...graphRunsQuery(graphId), select: (runs) => runs[0] });
  useEffect(() => {
    if (latest && !useGraphStore.getState().run) useGraphStore.getState().runUpdated(latest);
  }, [latest]);
}
