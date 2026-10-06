import { useQueryClient } from '@tanstack/react-query';
import { useEffect, type ReactNode } from 'react';

import { refreshSession } from '@/lib/api/session';
import { useAuthStore } from '@/stores/auth-store';
import { startQueryPersistence } from '@/lib/query-persist';

/** Refresh one minute before the access token expires. */
const REFRESH_LEAD_MS = 60_000;

/**
 * Restores the session from the refresh cookie on boot, keeps the access token fresh, and drops all
 * cached server state whenever the user signs out (explicitly or because refresh failed).
 */
export function AuthBootstrap({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient();
  const expiresAt = useAuthStore((s) => s.expiresAt);

  useEffect(() => {
    if (useAuthStore.getState().status === 'booting') void refreshSession();
  }, []);

  useEffect(() => {
    if (!expiresAt) return;
    const timer = setTimeout(
      () => void refreshSession(),
      Math.max(5_000, expiresAt - Date.now() - REFRESH_LEAD_MS),
    );
    return () => clearTimeout(timer);
  }, [expiresAt]);

  useEffect(
    () =>
      useAuthStore.subscribe((state, prev) => {
        if (prev.status === 'authenticated' && state.status === 'anonymous') queryClient.clear();
      }),
    [queryClient],
  );

  // Lists are kept in this browser between visits, per account (see query-persist).
  useEffect(() => startQueryPersistence(queryClient), [queryClient]);

  return children;
}
