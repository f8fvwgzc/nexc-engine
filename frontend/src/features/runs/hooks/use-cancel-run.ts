import { useMutation, useQueryClient } from '@tanstack/react-query';

import { qk } from '@/lib/query-keys';
import { cancelRun } from '@/features/runs/api';
import { useGraphStore } from '@/stores/graph-store';

export function useCancelRun() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: cancelRun,
    meta: { successMessage: 'Run cancelled' },
    onSuccess: (run) => {
      const store = useGraphStore.getState();
      if (store.run?.id === run.id) store.runUpdated(run);
      queryClient.setQueryData(qk.runs.detail(run.id), run);
      void queryClient.invalidateQueries({ queryKey: qk.graphs.runs(run.graph_id) });
    },
  });
}
