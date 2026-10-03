import { Navigate, Outlet, useLocation } from 'react-router-dom';

import { useAuthStore } from '@/stores/auth-store';

import { BootSplash } from './boot-splash';

/** Gate for /app/*: waits for session restore, then redirects anonymous users to /login?next=… */
export function PrivateRoute() {
  const status = useAuthStore((s) => s.status);
  const location = useLocation();

  if (status === 'booting') return <BootSplash />;
  if (status === 'anonymous') {
    const next = encodeURIComponent(`${location.pathname}${location.search}${location.hash}`);
    return <Navigate to={`/login?next=${next}`} replace />;
  }
  return <Outlet />;
}
