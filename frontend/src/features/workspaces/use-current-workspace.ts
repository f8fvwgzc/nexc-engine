import { useQuery } from '@tanstack/react-query';
import { useEffect } from 'react';

import type { Workspace } from '@/schemas/workspace';
import { useWorkspaceStore } from '@/stores/workspace-store';

import { workspacesQuery } from './api';

/**
 * The workspace the user is working in. Falls back to their first workspace when none was
 * chosen yet or the remembered one is gone (they left it, or it was deleted).
 */
export function useCurrentWorkspace(): { current: Workspace | undefined; all: Workspace[] } {
  const { data: all = [] } = useQuery(workspacesQuery());
  const currentId = useWorkspaceStore((s) => s.currentId);
  const setCurrent = useWorkspaceStore((s) => s.setCurrent);
  const current = all.find((w) => w.id === currentId) ?? all[0];
  useEffect(() => {
    if (current && current.id !== currentId) setCurrent(current.id);
  }, [current, currentId, setCurrent]);
  return { current, all };
}

/** Id of the open workspace, if known yet. Lists are scoped to it. */
export function useWorkspaceId(): string | undefined {
  return useCurrentWorkspace().current?.id;
}
