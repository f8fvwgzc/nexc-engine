/**
 * How long the data of each kind of resource stays fresh in the query cache (its `staleTime`).
 * While it is fresh, opening a page again or coming back to the tab reads the cache instead of
 * asking the server; once it is stale, the next mount or focus refetches it in the background,
 * as a conditional GET (a 304 when nothing changed, CONTRACT §3). Live state polls on its own
 * interval instead of waiting for a focus.
 */
export const FRESH = {
  /** Built-in catalogues that change with a release: graph and role templates, the audit's questions. */
  catalogue: 30 * 60_000,
  /**
   * The workspace's setup, changed by an admin now and then: workspaces, members, invites, teams,
   * workflows, labels, cycles, settings, guardrails, policies, servers, counterparts, destinations.
   */
  setup: 5 * 60_000,
  /** What people work through: graphs, issues, projects, agents, runs, sessions, documents. */
  list: 60_000,
  /** Things that change as agents work and are also polled: the inbox, approvals, alerts. */
  live: 10_000,
} as const;
