import { useMutation, useQueryClient } from '@tanstack/react-query';

import { qk } from '@/lib/query-keys';

import { updateLlmSettings } from '../api';

export function useUpdateLlmSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: updateLlmSettings,
    meta: { errorToast: false },
    onSuccess: (settings) => {
      queryClient.setQueryData(qk.settings.llm, settings);
      // demo_mode may have flipped.
      void queryClient.invalidateQueries({ queryKey: qk.orchestrator });
    },
  });
}
