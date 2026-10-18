import { useQuery } from '@tanstack/react-query';
import { useEffect } from 'react';

import { graphRunsQuery } from '@/features/runs/api';
import { useGraphStore } from '@/stores/graph-store';

/** Seeds the live run state from the newest run, so a reload mid-run keeps showing progress. */
export function useHydrateLatestRun(graphId: string) {
  const { data: latest } = useQuery({
    ...graphRunsQuery(graphId),
    select: (runs) => runs[0],
    // The graph's event stream keeps its runs current on the canvas: no need to poll them, or
    // to read them again on focus, while one runs.
    refetchInterval: false,
    refetchOnWindowFocus: false,
  });
  useEffect(() => {
    if (latest && !useGraphStore.getState().run) useGraphStore.getState().runUpdated(latest);
  }, [latest]);
}
