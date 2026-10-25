import { QueryClient } from '@tanstack/react-query';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { routeModules } from '@/app/route-modules';
import { qk } from '@/lib/query-keys';

import { onIntent, prefetchIssue, routeChunk } from './prefetch';

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
  vi.clearAllMocks();
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
