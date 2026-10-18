import { useMutation, useQueryClient } from '@tanstack/react-query';

import { qk } from '@/lib/query-keys';

import { cancelAgent } from '../api';

/** Stops one sub-agent of a run; the tree is read again once the stop is recorded. */
export function useCancelAgent(runId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ spawnSpanId, reason }: { spawnSpanId: string; reason?: string }) =>
      cancelAgent(runId, spawnSpanId, reason),
    meta: { successMessage: 'The sub-agent is being stopped; the run goes on' },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: qk.sessions.detail(runId) });
    },
  });
}
