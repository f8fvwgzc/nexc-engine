import { QueryClient } from '@tanstack/react-query';
import type { LoaderFunctionArgs } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { routeModules } from '@/app/route-modules';
import { queryClient } from '@/lib/query-client';
import { qk } from '@/lib/query-keys';
import type { Workspace } from '@/schemas/workspace';
import { useAuthStore } from '@/stores/auth-store';
import { useWorkspaceStore } from '@/stores/workspace-store';
import * as f from '@/test/fixtures';

import { onIntent, pageLoaders, prefetchIssue, routeChunk } from './prefetch';

// The pages themselves are not loaded: each loader is a stub that says it was asked.
vi.mock('@/app/route-modules', () => {
  const stubs: Record<string, () => Promise<unknown>> = {};
  return {
    routeModules: new Proxy(stubs, {
      get: (target, name: string) => (target[name] ??= vi.fn(() => Promise.resolve({}))),
    }),
  };
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.clearAllMocks();
  queryClient.clear();
  useAuthStore.getState().clearSession();
});

describe('the page a link opens', () => {
  it('is found by its address, with or without a search', () => {
    expect(routeChunk('/app')).toBe(routeModules.dashboard);
    expect(routeChunk('/app/issues')).toBe(routeModules.issues);
    expect(routeChunk('/app/issues?team=t')).toBe(routeModules.issues);
    expect(routeChunk('/app/issues/0199')).toBe(routeModules.issue);
    expect(routeChunk('/app/graphs/g')).toBe(routeModules.graph);
    expect(routeChunk('/app/settings/members')).toBe(routeModules.members);
    expect(routeChunk('/app/settings/alert-destinations')).toBe(
      routeModules.settingsAlertDestinations,
    );
    expect(routeChunk('/app/platform/users')).toBe(routeModules.platformUsers);
  });

  it('is nothing for an address the app does not serve', () => {
    expect(routeChunk('/login')).toBeUndefined();
    expect(routeChunk('/app/settings/nothing-here')).toBeUndefined();
    expect(routeChunk('/app/nowhere')).toBeUndefined();
  });
});

describe('the intent to follow a link', () => {
  it('counts once the pointer rests on it, not when it passes over', () => {
    vi.useFakeTimers();
    const load = vi.fn();
    const passing = onIntent(load);
    passing.onPointerEnter();
    passing.onPointerLeave();
    vi.advanceTimersByTime(500);
    expect(load).not.toHaveBeenCalled();

    const resting = onIntent(load);
    resting.onPointerEnter();
    vi.advanceTimersByTime(100);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('counts at once when the keyboard focuses it', () => {
    const load = vi.fn();
    onIntent(load).onFocus();
    expect(load).toHaveBeenCalledTimes(1);
  });
});

describe('an issue about to be opened', () => {
  it('loads its page, the issue, its timeline and its team’s workflow', () => {
    const client = new QueryClient();
    const prefetch = vi.spyOn(client, 'prefetchQuery').mockResolvedValue(undefined);
    prefetchIssue(client, { id: 'i', workspace_id: 'w', team_id: 't' });
    expect(routeModules.issue).toHaveBeenCalled();
    expect(prefetch.mock.calls.map(([options]) => options.queryKey)).toEqual([
      qk.issues.one('i'),
      qk.issues.events('i'),
      qk.issues.states('w', 't'),
    ]);
  });
});

describe('the members page loading ahead', () => {
  const workspace = (role: Workspace['role']): Workspace => ({
    id: '0199eeee-0000-7000-8000-000000000001',
    name: 'Acme',
    slug: 'acme',
    role,
    member_count: 2,
    created_at: '2026-10-20T09:00:00Z',
  });

  /** Opens the members page in `open` and says which reads its loader started. */
  async function readsFor(open: Workspace) {
    useAuthStore.getState().setSession(f.authResponse);
    useWorkspaceStore.setState({ currentId: open.id });
    queryClient.setQueryData(qk.workspaces.all, [open]);
    vi.spyOn(globalThis, 'fetch').mockRejectedValue(new TypeError('offline'));
    const ensure = vi.spyOn(queryClient, 'ensureQueryData');
    const args = {
      request: new Request('http://localhost/app/settings/members'),
      params: {},
      context: undefined,
    } as unknown as LoaderFunctionArgs;
    await pageLoaders.members(args);
    // The loader returns at once; the reads start once the workspace is known.
    await new Promise((resolve) => setTimeout(resolve, 0));
    return ensure.mock.calls.map(([options]) => options.queryKey);
  }

  it('reads the members for a member', async () => {
    const open = workspace('member');
    expect(await readsFor(open)).toContainEqual(qk.workspaces.members(open.id));
  });

  it('does not ask for them for a guest, whom the server refuses them', async () => {
    const open = workspace('guest');
    expect(await readsFor(open)).not.toContainEqual(qk.workspaces.members(open.id));
  });
});
