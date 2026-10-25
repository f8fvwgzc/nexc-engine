import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useRef } from 'react';

import { qk } from '@/lib/query-keys';
import type { IssueEvent, Reaction } from '@/schemas/issue';
import { useAuthStore } from '@/stores/auth-store';

import { addReaction, removeReaction } from './api';
import { toggleReaction } from './reactions';

/**
 * Adds the reader's reaction to an entry of the issue, or takes it back: the timeline shows it at
 * once, the server's answer then replaces the entry's reactions, and a failure undoes the change
 * (the error shows as a toast). While another toggle of the same entry is on its way, an answer
 * is not applied: the last one to land brings the state that counts.
 */
export function useReactions(issueId: string) {
  const queryClient = useQueryClient();
  const user = useAuthStore((s) => s.user);
  const pending = useRef(new Map<string, number>());
  const key = qk.issues.events(issueId);
  const patch = (eventId: string, change: (reactions: Reaction[]) => Reaction[]) =>
    queryClient.setQueryData<IssueEvent[]>(key, (events) =>
      events?.map((e) => (e.id === eventId ? { ...e, reactions: change(e.reactions) } : e)),
    );
  const toggle = useMutation({
    mutationFn: ({ eventId, emoji, on }: { eventId: string; emoji: string; on: boolean }) =>
      on ? addReaction(issueId, eventId, emoji) : removeReaction(issueId, eventId, emoji),
    onMutate: async ({ eventId, emoji, on }) => {
      pending.current.set(eventId, (pending.current.get(eventId) ?? 0) + 1);
      await queryClient.cancelQueries({ queryKey: key });
      if (user)
        patch(eventId, (r) => toggleReaction(r, emoji, on, { user_id: user.id, name: user.name }));
    },
    onSuccess: (reactions, { eventId }) => {
      if (pending.current.get(eventId) === 1) patch(eventId, () => reactions);
    },
    onError: (_error, { eventId, emoji, on }) => {
      if (user)
        patch(eventId, (r) => toggleReaction(r, emoji, !on, { user_id: user.id, name: user.name }));
    },
    onSettled: (_data, _error, { eventId }) => {
      const left = (pending.current.get(eventId) ?? 1) - 1;
      if (left > 0) pending.current.set(eventId, left);
      else pending.current.delete(eventId);
    },
  });
  return (eventId: string, emoji: string, on: boolean) => toggle.mutate({ eventId, emoji, on });
}
