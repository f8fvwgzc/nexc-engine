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
              { path: 'memory', element: <Navigate to="/app/settings/memory" replace /> },
              { path: 'inbox', ...page(routeModules.inbox, crumbs({ label: 'Inbox' })) },
              { path: 'issues', ...page(routeModules.issues, crumbs({ label: 'Issues' })) },
              { path: 'projects', ...page(routeModules.projects, crumbs({ label: 'Projects' })) },
              {
                path: 'projects/:projectId',
                ...page(
                  routeModules.project,
                  crumbs({ label: 'Projects', to: '/app/projects' }, { label: 'Project' }),
                ),
              },
              // These pages moved into the settings area; old addresses still land there.
              { path: 'teams', element: <Navigate to="/app/settings/teams" replace /> },
              { path: 'members', element: <Navigate to="/app/settings/members" replace /> },
              { path: 'usage', element: <Navigate to="/app/settings/usage" replace /> },
              { path: 'audit', element: <Navigate to="/app/settings/audit" replace /> },
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
              { path: 'infrastructure', ...page(routeModules.platformInfrastructure) },
              { path: '*', element: <Navigate to="/app/platform/workspaces" replace /> },
            ],
          },
          {
            // Settings are their own screen, with their own sidebar, in place of the app's.
            path: 'settings',
            ...page(routeModules.settingsShell),
            children: [
              { index: true, element: <Navigate to="/app/settings/profile" replace /> },
              { path: 'profile', ...page(routeModules.settingsProfile) },
              { path: 'preferences', ...page(routeModules.settingsPreferences) },
              { path: 'members', ...page(routeModules.members) },
              { path: 'teams', ...page(routeModules.teams) },
              { path: 'ai', ...page(routeModules.settingsAi) },
              { path: 'guardrails', ...page(routeModules.settingsGuardrails) },
              { path: 'memory', ...page(routeModules.memory) },
              { path: 'knowledge', ...page(routeModules.settingsKnowledge) },
              { path: 'usage', ...page(routeModules.usage) },
              { path: 'audit', ...page(routeModules.audit) },
              { path: 'activity', ...page(routeModules.settingsActivity) },
              { path: 'map', ...page(routeModules.settingsMap) },
              { path: 'transfer', ...page(routeModules.settingsTransfer) },
              {
                path: 'infrastructure',
                element: <Navigate to="/app/platform/infrastructure" replace />,
              },
              { path: '*', element: <Navigate to="/app/settings/profile" replace /> },
            ],
          },
        ],
      },
      { path: '*', ...page(routeModules.notFound) },
    ],
  },
];

export const router = createBrowserRouter(routes);
