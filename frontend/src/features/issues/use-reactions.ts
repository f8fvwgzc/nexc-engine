import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useRef } from 'react';

import { qk } from '@/lib/query-keys';
import type { IssueEvent, Reaction } from '@/schemas/issue';
import { useAuthStore } from '@/stores/auth-store';

import { addReaction, removeReaction } from './api';
import { toggleReaction } from './reactions';

/**
 * The toggles of one entry still on their way, and whether the activity is to be read again once
 * they are all back: two of them overlapped, or one called off a read of the activity.
 */
interface InFlight {
  pending: number;
  reread: boolean;
}

/**
 * Adds the reader's reaction to an entry of the issue, or takes it back: the timeline shows it at
 * once, the server's answer then replaces the entry's reactions, and a failure undoes the change
 * (the error shows as a toast). While toggles of one entry overlap, no answer or failure can tell
 * which state is the server's (they may land in any order), so none is applied: once the last one
 * is back, the activity is read again.
 */
export function useReactions(issueId: string) {
  const queryClient = useQueryClient();
  const user = useAuthStore((s) => s.user);
  const inFlight = useRef(new Map<string, InFlight>());
  const key = qk.issues.events(issueId);
  const patch = (eventId: string, change: (reactions: Reaction[]) => Reaction[]) =>
    queryClient.setQueryData<IssueEvent[]>(key, (events) =>
      events?.map((e) => (e.id === eventId ? { ...e, reactions: change(e.reactions) } : e)),
    );
  /** Whether a toggle's answer or failure may be applied: not once another one overlapped it. */
  const applies = (eventId: string) => !inFlight.current.get(eventId)?.reread;
  const toggle = useMutation({
    mutationFn: ({ eventId, emoji, on }: { eventId: string; emoji: string; on: boolean }) =>
      on ? addReaction(issueId, eventId, emoji) : removeReaction(issueId, eventId, emoji),
    onMutate: async ({ eventId, emoji, on }) => {
      // A read of the activity on its way is called off, so it cannot undo the change shown here.
      const calledOff = queryClient.isFetching({ queryKey: key }) > 0;
      const entry = inFlight.current.get(eventId) ?? { pending: 0, reread: false };
      inFlight.current.set(eventId, {
        pending: entry.pending + 1,
        reread: entry.reread || entry.pending > 0 || calledOff,
      });
      await queryClient.cancelQueries({ queryKey: key });
      if (user)
        patch(eventId, (r) => toggleReaction(r, emoji, on, { user_id: user.id, name: user.name }));
    },
    onSuccess: (reactions, { eventId }) => {
      if (applies(eventId)) patch(eventId, () => reactions);
    },
    onError: (_error, { eventId, emoji, on }) => {
      if (user && applies(eventId))
        patch(eventId, (r) => toggleReaction(r, emoji, !on, { user_id: user.id, name: user.name }));
    },
    onSettled: (_data, _error, { eventId }) => {
      const entry = inFlight.current.get(eventId);
      if (!entry) return;
      entry.pending -= 1;
      if (entry.pending > 0) return;
      inFlight.current.delete(eventId);
      if (entry.reread) void queryClient.invalidateQueries({ queryKey: key });
    },
  });
  return (eventId: string, emoji: string, on: boolean) => toggle.mutate({ eventId, emoji, on });
}
