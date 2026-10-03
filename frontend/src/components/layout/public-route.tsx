import { Navigate, Outlet, useSearchParams } from 'react-router-dom';

import { safeNextPath } from '@/lib/utils';
import { useAuthStore } from '@/stores/auth-store';

/**
 * Wrapper for /, /login and /register. Renders immediately (no boot splash, good for LCP) and
 * redirects into the app as soon as a session exists.
 */
export function PublicRoute() {
  const status = useAuthStore((s) => s.status);
  const [params] = useSearchParams();
  if (status === 'authenticated') return <Navigate to={safeNextPath(params.get('next'))} replace />;
  return <Outlet />;
}
