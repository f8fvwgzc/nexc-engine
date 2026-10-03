import { useMutation, useQueryClient } from '@tanstack/react-query';

import { qk } from '@/lib/query-keys';
import type { Agent, AgentInput } from '@/schemas/agent';

import { createAgent, deleteAgent, updateAgent } from '../api';

export function useSaveAgent() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, body }: { id: string | null; body: AgentInput }) =>
      id ? updateAgent(id, body) : createAgent(body),
    meta: { errorToast: false },
    onSuccess: (agent) =>
      queryClient.setQueryData<Agent[]>(qk.agents.all, (list) =>
        list
          ? list.some((a) => a.id === agent.id)
            ? list.map((a) => (a.id === agent.id ? agent : a))
            : [...list, agent]
          : list,
      ),
  });
}

export function useDeleteAgent() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteAgent,
    meta: { successMessage: 'Agent deleted' },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: qk.agents.all }),
  });
}
