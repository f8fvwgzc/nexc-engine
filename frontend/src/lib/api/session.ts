import { env } from '@/lib/env';
import { authResponseSchema, type AuthResponse } from '@/schemas/auth';
import { useAuthStore } from '@/stores/auth-store';

/** CSRF guard header required by cookie-authenticated endpoints (CONTRACT §3). */
export const CSRF_HEADERS = { 'X-Requested-With': 'nexc' } as const;

let inflight: Promise<AuthResponse | null> | null = null;

async function doRefresh(): Promise<AuthResponse | null> {
  try {
    const res = await fetch(`${env.apiBaseUrl}/auth/refresh`, {
      method: 'POST',
      credentials: 'include',
      headers: { Accept: 'application/json', ...CSRF_HEADERS },
    });
    if (!res.ok) {
      useAuthStore.getState().clearSession();
      return null;
    }
    const session = authResponseSchema.parse(await res.json());
    useAuthStore.getState().setSession(session);
    return session;
  } catch {
    useAuthStore.getState().clearSession();
    return null;
  }
}

/**
 * Exchanges the refresh cookie for a new access token. Single-flight: concurrent callers share one
 * request, which matters because the backend rotates the refresh token and treats reuse as theft.
 */
export function refreshSession(): Promise<AuthResponse | null> {
  inflight ??= doRefresh().finally(() => {
    inflight = null;
  });
  return inflight;
}
