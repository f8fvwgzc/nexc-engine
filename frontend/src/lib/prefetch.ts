import type { EnsureQueryDataOptions, QueryClient, QueryKey } from '@tanstack/react-query';
import type { LoaderFunction, LoaderFunctionArgs } from 'react-router-dom';

import { routeModules } from '@/app/route-modules';
import { agentsQuery } from '@/features/agents/api';
import { graphQuery, graphsQuery } from '@/features/graphs/api';
import {
  inboxQuery,
  issueEventsQuery,
  issueQuery,
  issuesQuery,
  labelsQuery,
  projectsQuery,
  statesQuery,
} from '@/features/issues/api';
import { alertsQuery } from '@/features/policies/api';
import { runQuery, runTraceQuery } from '@/features/runs/api';
import { secopsOverviewQuery } from '@/features/secops/api';
import { sessionQuery } from '@/features/sessions/api';
import {
  conversationsQuery,
  membersQuery,
  teamsQuery,
  usageQuery,
  workspacesQuery,
} from '@/features/workspaces/api';
import { defaultWorkspace } from '@/features/workspaces/use-current-workspace';
import { queryClient } from '@/lib/query-client';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';
import { useAuthStore } from '@/stores/auth-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

/*
 * Loading ahead. A route's loader starts the reads its page makes first, so they run while the
 * page's code downloads instead of after it; a link the pointer rests on (or the keyboard
 * focuses) loads the code of the page it opens, and a row of a list the reads of the page it
 * opens. Nothing here waits or throws: a read that fails is the page's to show when it makes it.
 */

/** Starts a read the page will make, unless a fresh copy is kept; a kept stale one is renewed. */
function warm<TData, TKey extends QueryKey>(
  client: QueryClient,
  options: EnsureQueryDataOptions<TData, Error, TData, TKey>,
): void {
  client.ensureQueryData({ ...options, revalidateIfStale: true }).catch(() => undefined);
}

/** Whether someone is signed in, once the session is known (it is restored on boot). */
function signedIn(): Promise<boolean> {
  const { status } = useAuthStore.getState();
  if (status !== 'booting') return Promise.resolve(status === 'authenticated');
  return new Promise((resolve) => {
    const stop = useAuthStore.subscribe((state) => {
      if (state.status === 'booting') return;
      stop();
      resolve(state.status === 'authenticated');
    });
  });
}

/** Whether the one signed in works in workspaces (a platform administrator does not). */
async function member(): Promise<boolean> {
  return (await signedIn()) && useAuthStore.getState().user?.role !== 'admin';
}

/**
 * The workspace the app's pages open in: the remembered one, or the one useCurrentWorkspace
 * picks when none is. None for a platform administrator, who works in the console.
 */
async function openWorkspace(client: QueryClient): Promise<Workspace | undefined> {
  if (!(await member())) return undefined;
  const all = await client.ensureQueryData(workspacesQuery());
  const remembered = useWorkspaceStore.getState().currentId;
  return all.find((w) => w.id === remembered) ?? defaultWorkspace(all);
}

type WarmPage = (client: QueryClient, workspace: Workspace, args: LoaderFunctionArgs) => void;

/** A loader that starts the page's first reads in the open workspace and returns at once. */
function inWorkspace(warmPage: WarmPage): LoaderFunction {
  return (args) => {
    openWorkspace(queryClient)
      .then((workspace) => workspace && warmPage(queryClient, workspace, args))
      .catch(() => undefined);
    return null;
  };
}

/** A loader that starts reads which need no workspace: one run, one graph, one issue. */
function byAddress(warmPage: (client: QueryClient, args: LoaderFunctionArgs) => void) {
  const loader: LoaderFunction = (args) => {
    member()
      .then((yes) => yes && warmPage(queryClient, args))
      .catch(() => undefined);
    return null;
  };
  return loader;
}

/** The `?team=` a list is narrowed to, as the issues and projects pages read it. */
function teamParam(request: Request): string | undefined {
  return new URL(request.url).searchParams.get('team') || undefined;
}

/**
 * The first reads of each page, by its route. Each matches the query (key and all) the page
 * itself makes, or it would read twice; a page whose first read depends on what the reader picks
 * in it starts only what it reads whatever they pick.
 */
export const pageLoaders = {
  dashboard: inWorkspace((client, workspace) => warm(client, graphsQuery(workspace.id))),
  graph: byAddress((client, { params }) => {
    if (params.graphId) warm(client, graphQuery(params.graphId));
  }),
  runDetail: byAddress((client, { params }) => {
    if (!params.runId) return;
    warm(client, runQuery(params.runId));
    warm(client, runTraceQuery(params.runId));
  }),
  session: byAddress((client, { params }) => {
    if (!params.runId) return;
    warm(client, sessionQuery(params.runId));
    warm(client, runTraceQuery(params.runId));
  }),
  agents: inWorkspace((client, workspace) => warm(client, agentsQuery(workspace.id))),
  secops: inWorkspace((client, workspace) => {
    if (workspace.role === 'guest') return;
    warm(client, secopsOverviewQuery(workspace.id));
    warm(client, agentsQuery(workspace.id));
  }),
  conversations: inWorkspace((client, workspace) => warm(client, conversationsQuery(workspace.id))),
  inbox: inWorkspace((client, workspace) => warm(client, inboxQuery(workspace.id))),
  issues: inWorkspace((client, workspace, { request }) => {
    warm(client, teamsQuery(workspace.id));
    warm(client, labelsQuery(workspace.id));
    // The list as it first opens: open issues, of the team in the address or of every team.
    warm(client, issuesQuery(workspace.id, { team_id: teamParam(request), open: true }));
  }),
  issue: byAddress((client, { params }) => {
    if (params.issueId) warmIssue(client, params.issueId);
  }),
  myIssues: inWorkspace((client, workspace, { request }) => {
    const me = useAuthStore.getState().user?.id;
    warm(client, teamsQuery(workspace.id));
    warm(client, labelsQuery(workspace.id));
    if (me) {
      warm(
        client,
        issuesQuery(workspace.id, { team_id: teamParam(request), assignee_id: me, open: true }),
      );
    }
  }),
  projects: inWorkspace((client, workspace, { request }) => {
    warm(client, teamsQuery(workspace.id));
    warm(client, projectsQuery(workspace.id, teamParam(request)));
    if (workspace.role !== 'guest') warm(client, membersQuery(workspace.id));
  }),
  project: inWorkspace((client, workspace, { params }) => {
    warm(client, projectsQuery(workspace.id));
    if (params.projectId) warmProjectIssues(client, workspace.id, params.projectId);
  }),
  members: inWorkspace((client, workspace) => warm(client, membersQuery(workspace.id))),
  teams: inWorkspace((client, workspace) => warm(client, teamsQuery(workspace.id))),
  // The page opens on the last 30 days.
  usage: inWorkspace((client, workspace) => warm(client, usageQuery(workspace.id, 30))),
  alerts: inWorkspace((client, workspace) => {
    if (isWorkspaceAdmin(workspace.role)) warm(client, alertsQuery(workspace.id));
  }),
} satisfies Record<string, LoaderFunction>;

export type PageLoaders = typeof pageLoaders;

/** The reads an issue's page makes first: the issue, its timeline and its team's workflow. */
function warmIssue(client: QueryClient, issueId: string): void {
  warm(client, issueEventsQuery(issueId));
  client
    .ensureQueryData({ ...issueQuery(issueId), revalidateIfStale: true })
    .then((issue) => warm(client, statesQuery(issue.workspace_id, issue.team_id)))
    .catch(() => undefined);
}

/** The issues a project's page lists first: its open ones, of every team. */
function warmProjectIssues(client: QueryClient, workspaceId: string, projectId: string): void {
  warm(client, teamsQuery(workspaceId));
  warm(client, labelsQuery(workspaceId));
  warm(client, issuesQuery(workspaceId, { project_id: projectId, open: true }));
}

/**
 * An issue row or card the reader is about to open: its page's code, the issue, its timeline
 * and its team's workflow. The row already knows the workspace and team, so they load together.
 */
export function prefetchIssue(
  client: QueryClient,
  issue: { id: string; workspace_id: string; team_id: string },
): void {
  void routeModules.issue();
  void client.prefetchQuery(issueQuery(issue.id));
  void client.prefetchQuery(issueEventsQuery(issue.id));
  void client.prefetchQuery(statesQuery(issue.workspace_id, issue.team_id));
}

/** A project row the reader is about to open: its page's code, the projects and its issues. */
export function prefetchProject(client: QueryClient, workspaceId: string, projectId: string): void {
  void routeModules.project();
  void client.prefetchQuery(projectsQuery(workspaceId));
  void client.prefetchQuery(issuesQuery(workspaceId, { project_id: projectId, open: true }));
}

type Chunk = () => Promise<unknown>;

/** The pages under /app/settings and /app/platform, by the last part of their address. */
const SETTINGS_CHUNKS: Record<string, Chunk> = {
  profile: routeModules.settingsProfile,
  preferences: routeModules.settingsPreferences,
  members: routeModules.members,
  teams: routeModules.teams,
  ai: routeModules.settingsAi,
  guardrails: routeModules.settingsGuardrails,
  policies: routeModules.settingsPolicies,
  alerts: routeModules.settingsAlerts,
  'alert-destinations': routeModules.settingsAlertDestinations,
  'mcp-servers': routeModules.settingsMcpServers,
  'a2a-counterparts': routeModules.settingsA2aCounterparts,
  memory: routeModules.memory,
  knowledge: routeModules.settingsKnowledge,
  usage: routeModules.usage,
  audit: routeModules.audit,
  activity: routeModules.settingsActivity,
  map: routeModules.settingsMap,
  ontology: routeModules.settingsOntology,
  transfer: routeModules.settingsTransfer,
};
const PLATFORM_CHUNKS: Record<string, Chunk> = {
  workspaces: routeModules.platformWorkspaces,
  users: routeModules.platformUsers,
  activity: routeModules.platformActivity,
  account: routeModules.platformAccount,
  infrastructure: routeModules.platformInfrastructure,
};
/** The other pages of the app: a list, and the page of one of its items. */
const APP_CHUNKS: Record<string, [list: Chunk, item?: Chunk]> = {
  graphs: [routeModules.dashboard, routeModules.graph],
  runs: [routeModules.runs, routeModules.runDetail],
  agents: [routeModules.agents],
  secops: [routeModules.secops],
  sessions: [routeModules.sessions, routeModules.session],
  inbox: [routeModules.inbox],
  issues: [routeModules.issues, routeModules.issue],
  'my-issues': [routeModules.myIssues],
  projects: [routeModules.projects, routeModules.project],
  conversations: [routeModules.conversations],
  artifacts: [routeModules.artifacts, routeModules.artifactFile],
};

/** The code of the page at `to` (an address inside the app), if it is one of the router's. */
export function routeChunk(to: string): Chunk | undefined {
  const [, app, section, rest] = to.split(/[?#]/)[0]!.split('/');
  if (app !== 'app') return undefined;
  if (section === undefined || section === '') return routeModules.dashboard;
  if (section === 'settings') return rest ? SETTINGS_CHUNKS[rest] : undefined;
  if (section === 'platform') return rest ? PLATFORM_CHUNKS[rest] : undefined;
  const chunks = APP_CHUNKS[section];
  return rest ? chunks?.[1] : chunks?.[0];
}

/** Loads the code of the page a link opens, so following it does not wait for the download. */
export function prefetchRoute(to: string): void {
  void routeChunk(to)?.();
}

/** How long a pointer rests on a link before it counts as meaning to follow it. */
const HOVER_INTENT_MS = 80;
/** The hover waiting to count: the pointer is over one link at a time. */
let hovering: ReturnType<typeof setTimeout> | undefined;

/**
 * Handlers that run `load` when the reader means to follow a link: the pointer rests on it (one
 * passing over a list loads nothing), or the keyboard focuses it.
 */
export function onIntent(load: () => void) {
  return {
    onPointerEnter: () => {
      clearTimeout(hovering);
      hovering = setTimeout(load, HOVER_INTENT_MS);
    },
    onPointerLeave: () => {
      clearTimeout(hovering);
      hovering = undefined;
    },
    onFocus: load,
  };
}

/** {@link onIntent} handlers for a link inside the app: they load the code of its page. */
export function prefetchRouteOnIntent(to: string) {
  return onIntent(() => prefetchRoute(to));
}
