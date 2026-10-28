import { hashKey, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';

import { unreadCountQuery } from '@/features/issues/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { qk } from '@/lib/query-keys';

/**
 * How many unread notifications the member has in the open workspace: a count from the server,
 * not the inbox itself. Whatever refreshes the inbox refreshes the count (its key is under the
 * inbox's); a new inbox written straight into the cache (marking read) asks for the count again.
 */
export function useUnread(): number {
  const { current } = useCurrentWorkspace();
  const workspaceId = current?.id ?? '';
  const queryClient = useQueryClient();
  const { data } = useQuery({ ...unreadCountQuery(workspaceId), enabled: !!current });

  useEffect(() => {
    if (!workspaceId) return;
    const inbox = hashKey(qk.issues.inbox(workspaceId));
    return queryClient.getQueryCache().subscribe((event) => {
      // Only an inbox set by hand: one refetched by an invalidation of its key has the count
      // refetching beside it already.
      if (
        event.query.queryHash === inbox &&
        event.type === 'updated' &&
        event.action.type === 'success' &&
        event.action.manual
      ) {
        void queryClient.invalidateQueries({ queryKey: qk.issues.unread(workspaceId) });
      }
    });
  }, [queryClient, workspaceId]);

  return data ?? 0;
}
