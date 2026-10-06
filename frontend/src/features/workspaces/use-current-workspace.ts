import { useQuery } from '@tanstack/react-query';
import { useEffect } from 'react';

import type { Workspace } from '@/schemas/workspace';
import { useWorkspaceStore } from '@/stores/workspace-store';

import { workspacesQuery } from './api';

/**
 * The workspace to open when the user has not chosen one: the one with the most members, the
 * first of them on a tie. Someone invited to a team's workspace also has a personal one of
 * their own, older and empty; the shared one is where their work is.
 */
export function defaultWorkspace(all: Workspace[]): Workspace | undefined {
  return all.reduce<Workspace | undefined>(
    (best, w) => (best === undefined || w.member_count > best.member_count ? w : best),
    undefined,
  );
}

/**
 * The workspace the user is working in. Falls back to {@link defaultWorkspace} when none was
 * chosen yet or the remembered one is gone (they left it, it was deleted, or another account
 * used this browser before).
 */
export function useCurrentWorkspace(): { current: Workspace | undefined; all: Workspace[] } {
  const { data: all = [] } = useQuery(workspacesQuery());
  const currentId = useWorkspaceStore((s) => s.currentId);
  const setCurrent = useWorkspaceStore((s) => s.setCurrent);
  const current = all.find((w) => w.id === currentId) ?? defaultWorkspace(all);
  useEffect(() => {
    if (current && current.id !== currentId) setCurrent(current.id);
  }, [current, currentId, setCurrent]);
  return { current, all };
}

/** Id of the open workspace, if known yet. Lists are scoped to it. */
export function useWorkspaceId(): string | undefined {
  return useCurrentWorkspace().current?.id;
}
