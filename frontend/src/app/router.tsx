import type { ComponentType } from 'react';
import {
  createBrowserRouter,
  Navigate,
  type NonIndexRouteObject,
  type RouteObject,
} from 'react-router-dom';

import { BootSplash } from '@/components/layout/boot-splash';
import { PrivateRoute } from '@/components/layout/private-route';
import { PublicLayout } from '@/components/layout/public-layout';
import { PublicRoute } from '@/components/layout/public-route';
import { RootLayout } from '@/components/layout/root-layout';
import { RouteError } from '@/components/layout/route-error';
import type { RouteHandle } from '@/components/layout/route-handle';

import { routeModules } from './route-modules';

type PageRoute = Pick<NonIndexRouteObject, 'errorElement' | 'lazy' | 'handle'>;

/** Lazy route from a module with a default-exported component, with a per-route error boundary. */
function page(load: () => Promise<{ default: ComponentType }>, handle?: RouteHandle): PageRoute {
  return {
    handle,
    errorElement: <RouteError />,
    lazy: async () => ({ Component: (await load()).default }),
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
): PageRoute {
  return page(load, crumbs(parent, { label: title }));
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
              { index: true, ...page(routeModules.dashboard, crumbs({ label: 'Graphs' })) },
              {
                path: 'graphs/:graphId',
                ...page(
                  routeModules.graph,
                  crumbs({ label: 'Graphs', to: '/app' }, { dynamic: 'graph' }),
                ),
              },
              { path: 'graphs', element: <Navigate to="/app" replace /> },
              { path: 'runs', ...page(routeModules.runs, crumbs({ label: 'Runs' })) },
              {
                path: 'runs/:runId',
                ...page(
                  routeModules.runDetail,
                  crumbs({ label: 'Runs', to: '/app/runs' }, { dynamic: 'run' }),
                ),
              },
              { path: 'agents', ...page(routeModules.agents, crumbs({ label: 'Agents' })) },
              { path: 'secops', ...page(routeModules.secops, crumbs({ label: 'Trust' })) },
              { path: 'sessions', ...page(routeModules.sessions, crumbs({ label: 'Sessions' })) },
              {
                path: 'sessions/:runId',
                ...page(
                  routeModules.session,
                  crumbs({ label: 'Sessions', to: '/app/sessions' }, { dynamic: 'run' }),
                ),
              },
              { path: 'memory', element: <Navigate to="/app/settings/memory" replace /> },
              { path: 'inbox', ...page(routeModules.inbox, crumbs({ label: 'Inbox' })) },
              { path: 'issues', ...page(routeModules.issues, crumbs({ label: 'Issues' })) },
              {
                path: 'issues/:issueId',
                ...page(
                  routeModules.issue,
                  crumbs({ label: 'Issues', to: '/app/issues' }, { dynamic: 'issue' }),
                ),
              },
              {
                path: 'my-issues',
                ...page(routeModules.myIssues, crumbs({ label: 'My issues' })),
              },
              { path: 'projects', ...page(routeModules.projects, crumbs({ label: 'Projects' })) },
              {
                path: 'conversations',
                ...page(routeModules.conversations, crumbs({ label: 'Conversations' })),
              },
              {
                path: 'projects/:projectId',
                ...page(
                  routeModules.project,
                  crumbs({ label: 'Projects', to: '/app/projects' }, { dynamic: 'project' }),
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
                  { path: 'members', ...settingsPage(routeModules.members, 'Members') },
                  { path: 'teams', ...settingsPage(routeModules.teams, 'Teams') },
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
                    ...settingsPage(routeModules.settingsAlerts, 'Alerts', TRUST_CRUMB),
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
                  { path: 'usage', ...settingsPage(routeModules.usage, 'Usage') },
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
