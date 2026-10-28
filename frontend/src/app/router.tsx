import type { ComponentType } from 'react';
import {
  createBrowserRouter,
  Navigate,
  type NonIndexRouteObject,
  type RouteObject,
  type ShouldRevalidateFunction,
} from 'react-router-dom';

import { BootSplash } from '@/components/layout/boot-splash';
import { PrivateRoute } from '@/components/layout/private-route';
import { PublicLayout } from '@/components/layout/public-layout';
import { PublicRoute } from '@/components/layout/public-route';
import { RootLayout } from '@/components/layout/root-layout';
import { RouteError } from '@/components/layout/route-error';
import type { RouteHandle } from '@/components/layout/route-handle';
import type { PageLoaders } from '@/lib/prefetch';

import { routeModules } from './route-modules';

type PageRoute = Pick<NonIndexRouteObject, 'errorElement' | 'lazy' | 'handle' | 'shouldRevalidate'>;

/** A page whose route starts its first reads (see lib/prefetch.ts), by its loader's name. */
type Prefetched = keyof PageLoaders;

/** A search of the same page (a filter, a tab) reads what it needs itself. */
const onlyAnotherPage: ShouldRevalidateFunction = ({ currentUrl, nextUrl }) =>
  currentUrl.pathname !== nextUrl.pathname;

/**
 * Lazy route from a module with a default-exported component, with a per-route error boundary.
 * Given a loader, the route starts the page's first reads while its code downloads: the loader
 * comes from a small module of its own, in parallel with the page's, and returns at once.
 */
function page(
  load: () => Promise<{ default: ComponentType }>,
  handle?: RouteHandle,
  prefetch?: Prefetched,
): PageRoute {
  return {
    handle,
    errorElement: <RouteError />,
    lazy: {
      Component: async () => (await load()).default,
      ...(prefetch && {
        loader: async () => (await import('@/lib/prefetch')).pageLoaders[prefetch],
      }),
    },
    ...(prefetch && { shouldRevalidate: onlyAnotherPage }),
  };
}

const crumbs = (...list: RouteHandle['crumbs']): RouteHandle => ({ crumbs: list });

const SETTINGS_CRUMB = { label: 'Settings', to: '/app/settings' };
const TRUST_CRUMB = { label: 'Trust', to: '/app/secops' };

/** A settings page, under the core menu it is listed in: Settings, or Trust for the trust pages. */
function settingsPage(
  load: () => Promise<{ default: ComponentType }>,
  title: string,
  parent = SETTINGS_CRUMB,
  prefetch?: Prefetched,
): PageRoute {
  return page(load, crumbs(parent, { label: title }), prefetch);
}

export const routes: RouteObject[] = [
  {
    element: <RootLayout />,
    errorElement: <RouteError />,
    hydrateFallbackElement: <BootSplash />,
    children: [
      {
        element: <PublicRoute />,
        children: [
          { index: true, ...page(routeModules.landing) },
          {
            element: <PublicLayout />,
            children: [
              { path: 'login', ...page(routeModules.login) },
              { path: 'register', ...page(routeModules.register) },
            ],
          },
        ],
      },
      {
        // A reset link opens whoever is signed in on this browser, or nobody.
        element: <PublicLayout />,
        children: [{ path: 'reset-password', ...page(routeModules.resetPassword) }],
      },
      {
        path: 'app',
        element: <PrivateRoute />,
        children: [
          {
            ...page(routeModules.appShell),
            children: [
              {
                index: true,
                ...page(routeModules.dashboard, crumbs({ label: 'Graphs' }), 'dashboard'),
              },
              {
                path: 'graphs/:graphId',
                ...page(
                  routeModules.graph,
                  crumbs({ label: 'Graphs', to: '/app' }, { dynamic: 'graph' }),
                  'graph',
                ),
              },
              { path: 'graphs', element: <Navigate to="/app" replace /> },
              { path: 'runs', ...page(routeModules.runs, crumbs({ label: 'Runs' })) },
              {
                path: 'runs/:runId',
                ...page(
                  routeModules.runDetail,
                  crumbs({ label: 'Runs', to: '/app/runs' }, { dynamic: 'run' }),
                  'runDetail',
                ),
              },
              {
                path: 'agents',
                ...page(routeModules.agents, crumbs({ label: 'Agents' }), 'agents'),
              },
              {
                path: 'skills',
                ...page(
                  routeModules.skills,
                  crumbs({ label: 'Agents', to: '/app/agents' }, { label: 'Skills' }),
                ),
              },
              {
                path: 'secops',
                ...page(routeModules.secops, crumbs({ label: 'Trust' }), 'secops'),
              },
              { path: 'sessions', ...page(routeModules.sessions, crumbs({ label: 'Sessions' })) },
              {
                path: 'sessions/:runId',
                ...page(
                  routeModules.session,
                  crumbs({ label: 'Sessions', to: '/app/sessions' }, { dynamic: 'run' }),
                  'session',
                ),
              },
              { path: 'memory', element: <Navigate to="/app/settings/memory" replace /> },
              { path: 'inbox', ...page(routeModules.inbox, crumbs({ label: 'Inbox' }), 'inbox') },
              {
                path: 'issues',
                ...page(routeModules.issues, crumbs({ label: 'Issues' }), 'issues'),
              },
              {
                path: 'issues/:issueId',
                ...page(
                  routeModules.issue,
                  crumbs({ label: 'Issues', to: '/app/issues' }, { dynamic: 'issue' }),
                  'issue',
                ),
              },
              {
                path: 'my-issues',
                ...page(routeModules.myIssues, crumbs({ label: 'My issues' }), 'myIssues'),
              },
              {
                path: 'projects',
                ...page(routeModules.projects, crumbs({ label: 'Projects' }), 'projects'),
              },
              {
                path: 'conversations',
                ...page(
                  routeModules.conversations,
                  crumbs({ label: 'Conversations' }),
                  'conversations',
                ),
              },
              {
                path: 'projects/:projectId',
                ...page(
                  routeModules.project,
                  crumbs({ label: 'Projects', to: '/app/projects' }, { dynamic: 'project' }),
                  'project',
                ),
              },
              {
                path: 'artifacts',
                ...page(routeModules.artifacts, crumbs({ label: 'Artifacts' })),
              },
              {
                path: 'artifacts/:fileId',
                ...page(
                  routeModules.artifactFile,
                  crumbs({ label: 'Artifacts', to: '/app/artifacts' }, { dynamic: 'artifact' }),
                ),
              },
              // These pages moved into the settings area; old addresses still land there.
              { path: 'teams', element: <Navigate to="/app/settings/teams" replace /> },
              { path: 'members', element: <Navigate to="/app/settings/members" replace /> },
              { path: 'usage', element: <Navigate to="/app/settings/usage" replace /> },
              { path: 'audit', element: <Navigate to="/app/settings/audit" replace /> },
              {
                // The settings pages, in the shell like every other page; the panel lists them
                // under Settings, and the trust pages under Trust.
                path: 'settings',
                children: [
                  { index: true, element: <Navigate to="/app/settings/profile" replace /> },
                  { path: 'profile', ...settingsPage(routeModules.settingsProfile, 'Profile') },
                  {
                    path: 'preferences',
                    ...settingsPage(routeModules.settingsPreferences, 'Preferences'),
                  },
                  {
                    path: 'members',
                    ...settingsPage(routeModules.members, 'Members', SETTINGS_CRUMB, 'members'),
                  },
                  {
                    path: 'teams',
                    ...settingsPage(routeModules.teams, 'Teams', SETTINGS_CRUMB, 'teams'),
                  },
                  { path: 'ai', ...settingsPage(routeModules.settingsAi, 'AI accounts') },
                  {
                    path: 'guardrails',
                    ...settingsPage(routeModules.settingsGuardrails, 'Guardrails', TRUST_CRUMB),
                  },
                  {
                    path: 'policies',
                    ...settingsPage(routeModules.settingsPolicies, 'Policies', TRUST_CRUMB),
                  },
                  {
                    path: 'alerts',
                    ...settingsPage(routeModules.settingsAlerts, 'Alerts', TRUST_CRUMB, 'alerts'),
                  },
                  {
                    path: 'alert-destinations',
                    ...settingsPage(
                      routeModules.settingsAlertDestinations,
                      'Alert destinations',
                      TRUST_CRUMB,
                    ),
                  },
                  {
                    path: 'mcp-servers',
                    ...settingsPage(routeModules.settingsMcpServers, 'MCP servers'),
                  },
                  {
                    path: 'a2a-counterparts',
                    ...settingsPage(routeModules.settingsA2aCounterparts, 'A2A counterparts'),
                  },
                  { path: 'memory', ...settingsPage(routeModules.memory, 'Memory') },
                  {
                    path: 'knowledge',
                    ...settingsPage(routeModules.settingsKnowledge, 'Knowledge'),
                  },
                  {
                    path: 'usage',
                    ...settingsPage(routeModules.usage, 'Usage', SETTINGS_CRUMB, 'usage'),
                  },
                  { path: 'audit', ...settingsPage(routeModules.audit, 'Audit log', TRUST_CRUMB) },
                  { path: 'activity', ...settingsPage(routeModules.settingsActivity, 'Activity') },
                  { path: 'map', ...settingsPage(routeModules.settingsMap, 'Workspace map') },
                  { path: 'ontology', ...settingsPage(routeModules.settingsOntology, 'Ontology') },
                  {
                    path: 'transfer',
                    ...settingsPage(routeModules.settingsTransfer, 'Data transfer'),
                  },
                  {
                    path: 'infrastructure',
                    element: <Navigate to="/app/platform/infrastructure" replace />,
                  },
                  { path: '*', element: <Navigate to="/app/settings/profile" replace /> },
                ],
              },
              { path: '*', ...page(routeModules.notFound, crumbs({ label: 'Not found' })) },
            ],
          },
          {
            // The platform console: the whole installation, for platform administrators.
            path: 'platform',
            ...page(routeModules.platformShell),
            children: [
              { index: true, element: <Navigate to="/app/platform/workspaces" replace /> },
              { path: 'workspaces', ...page(routeModules.platformWorkspaces) },
              { path: 'users', ...page(routeModules.platformUsers) },
              { path: 'activity', ...page(routeModules.platformActivity) },
              { path: 'account', ...page(routeModules.platformAccount) },
              { path: 'infrastructure', ...page(routeModules.platformInfrastructure) },
              { path: '*', element: <Navigate to="/app/platform/workspaces" replace /> },
            ],
          },
        ],
      },
      { path: '*', ...page(routeModules.notFound) },
    ],
  },
];

export const router = createBrowserRouter(routes);
