import { useMutation, useQueryClient } from '@tanstack/react-query';

import { qk } from '@/lib/query-keys';
import type { Memory } from '@/schemas/memory';

import { deleteMemory } from '../api';

export function useDeleteMemory() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteMemory,
    meta: { successMessage: 'Memory deleted' },
    onSuccess: (_void, memoryId) => {
      queryClient.setQueriesData<Memory[]>({ queryKey: qk.memories.all }, (list) =>
        list?.filter((m) => m.id !== memoryId),
      );
    },
  });
}
