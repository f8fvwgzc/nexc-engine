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
              { path: 'memory', ...page(routeModules.memory, crumbs({ label: 'Memory' })) },
              { path: 'teams', ...page(routeModules.teams, crumbs({ label: 'Teams' })) },
              { path: 'members', ...page(routeModules.members, crumbs({ label: 'Members' })) },
              {
                path: 'settings',
                ...page(routeModules.settings, crumbs({ label: 'Settings' })),
              },
              { path: '*', ...page(routeModules.notFound, crumbs({ label: 'Not found' })) },
            ],
          },
        ],
      },
      { path: '*', ...page(routeModules.notFound) },
    ],
  },
];

export const router = createBrowserRouter(routes);
