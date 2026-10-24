import type { Query } from '@tanstack/react-query';

/**
 * How long the data of each kind of resource stays fresh in the query cache (its `staleTime`).
 * While it is fresh, opening a page again or coming back to the tab reads the cache instead of
 * asking the server; once it is stale, the next mount refetches it in the background, as a
 * conditional GET (a 304 when nothing changed, CONTRACT §3), and so does coming back to the tab
 * for the kinds that change outside it (see `changesElsewhere`). Live state polls on its own
 * interval instead of waiting for a focus. A query that names no kind is a list.
 */
export const FRESH = {
  /** Built-in catalogues that change with a release: graph and role templates, the audit's questions. */
  catalogue: 30 * 60_000,
  /**
   * The workspace's setup, changed by an admin now and then: workspaces, members, invites, teams,
   * workflows, labels, cycles, settings, guardrails, policies, servers, counterparts, destinations.
   */
  setup: 5 * 60_000,
  /**
   * What people work through, and what their work adds up to: graphs, issues, projects, agents,
   * runs, sessions, documents, the usage report.
   */
  list: 60_000,
  /** Things that change as agents work and are also polled: the inbox, approvals, alerts. */
  live: 10_000,
} as const;

/**
 * Whether a stale query is fetched again when the tab regains focus (the client's default): only
 * the lists and live state, which other people and agents change while the tab is away. Setup and
 * catalogues (fresh for 5 minutes or more) wait for their next mount; what the console changes
 * itself it refreshes at once. A query that says otherwise keeps its own setting.
 */
export function changesElsewhere(query: Query): boolean {
  return query.observers.some(({ options }) => {
    const { staleTime } = options;
    if (typeof staleTime === 'function') return true;
    return staleTime !== 'static' && (staleTime ?? 0) < FRESH.setup;
  });
}
