/** Route module loaders, shared by the router and by hover-prefetching in the sidebar. */
export const routeModules = {
  landing: () => import('@/pages/landing-page'),
  login: () => import('@/pages/login-page'),
  register: () => import('@/pages/register-page'),
  appShell: () => import('@/components/layout/app-shell'),
  dashboard: () => import('@/pages/dashboard-page'),
  graph: () => import('@/pages/graph-page'),
  runs: () => import('@/pages/runs-page'),
  runDetail: () => import('@/pages/run-detail-page'),
  agents: () => import('@/pages/agents-page'),
  memory: () => import('@/pages/memory-page'),
  members: () => import('@/pages/members-page'),
  teams: () => import('@/pages/teams-page'),
  usage: () => import('@/pages/usage-page'),
  settings: () => import('@/pages/settings-page'),
  notFound: () => import('@/pages/not-found-page'),
};
