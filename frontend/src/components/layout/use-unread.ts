import { useQuery } from '@tanstack/react-query';

import { inboxQuery } from '@/features/issues/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';

/** How many unread notifications the member has in the open workspace. */
export function useUnread(): number {
  const { current } = useCurrentWorkspace();
  const { data } = useQuery({ ...inboxQuery(current?.id ?? ''), enabled: !!current });
  return data?.filter((n) => n.read_at === null).length ?? 0;
}
