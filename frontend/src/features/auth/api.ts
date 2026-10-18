import { queryOptions } from '@tanstack/react-query';

import { apiRequest, apiSend } from '@/lib/api/client';
import { CSRF_HEADERS } from '@/lib/api/session';
import { env } from '@/lib/env';
import { qk } from '@/lib/query-keys';
import {
  authResponseSchema,
  loginProvidersSchema,
  userSchema,
  type LoginInput,
  type RegisterInput,
} from '@/schemas/auth';

/** How one signs in here: a password or not, registration or not, single sign-on or not. */
export const providersQuery = () =>
  queryOptions({
    queryKey: ['auth', 'providers'] as const,
    queryFn: ({ signal }) =>
      apiRequest('/auth/providers', loginProvidersSchema, { auth: false, signal }),
    staleTime: 5 * 60_000,
  });

/**
 * Where "Continue with …" sends the browser: the backend's single sign-on, which comes back to
 * `next` (a path of this site) signed in, with the refresh cookie set.
 */
export function ssoLoginUrl(next: string): string {
  return `${env.apiBaseUrl}/auth/oidc/login?return_to=${encodeURIComponent(next)}`;
}

export function login(body: LoginInput) {
  return apiRequest('/auth/login', authResponseSchema, { method: 'POST', body, auth: false });
}

export function register(body: RegisterInput) {
  return apiRequest('/auth/register', authResponseSchema, { method: 'POST', body, auth: false });
}

/** Sets a new password with the token of a reset link; no sign-in is needed. */
export function resetPassword(token: string, newPassword: string) {
  return apiSend('/auth/password/reset', {
    method: 'POST',
    body: { token, new_password: newPassword },
    auth: false,
  });
}

export function logout() {
  return apiSend('/auth/logout', { method: 'POST', headers: CSRF_HEADERS });
}

export const meQuery = () =>
  queryOptions({
    queryKey: qk.me,
    queryFn: ({ signal }) => apiRequest('/auth/me', userSchema, { signal }),
    staleTime: 5 * 60_000,
  });
