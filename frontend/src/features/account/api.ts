import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { saveDownload } from '@/lib/api/download';
import { authResponseSchema, userSchema } from '@/schemas/auth';
import { idSchema, timestampSchema } from '@/schemas/common';

/** Changes the signed-in person's display name. */
export function updateProfile(name: string) {
  return apiRequest('/auth/me', userSchema, { method: 'PATCH', body: { name } });
}

/**
 * Changes the password. Every session ends, on every device; this one starts anew with the
 * session that comes back.
 */
export function changePassword(body: { current_password: string; new_password: string }) {
  return apiRequest('/auth/password', authResponseSchema, { method: 'POST', body });
}

// Outside the roots copied to browser storage.
export const sessionsQuery = () =>
  queryOptions({
    queryKey: ['account', 'sessions'] as const,
    queryFn: ({ signal }) =>
      apiRequest('/auth/sessions', z.object({ active: z.number().int() }), { signal }),
  });

export const accountEventSchema = z.object({
  id: idSchema,
  kind: z.enum([
    'registered',
    'signed_in',
    'sign_in_failed',
    'password_changed',
    'password_reset',
    'reset_link_issued',
    'sessions_ended',
    'suspended',
    'reactivated',
    'role_changed',
    'two_factor_enabled',
    'two_factor_disabled',
  ]),
  ip: z.string().nullable(),
  detail: z.string(),
  created_at: timestampSchema,
});
export type AccountEvent = z.infer<typeof accountEventSchema>;

/** What happened to the signed-in person's access lately, newest first. */
export const activityQuery = () =>
  queryOptions({
    queryKey: ['account', 'activity'] as const,
    queryFn: ({ signal }) =>
      apiRequest('/auth/activity', z.array(accountEventSchema), { query: { limit: 30 }, signal }),
  });

export const twoFactorQuery = () =>
  queryOptions({
    queryKey: ['account', 'two-factor'] as const,
    queryFn: ({ signal }) =>
      apiRequest(
        '/auth/2fa',
        z.object({ enabled: z.boolean(), recovery_codes_left: z.number().int() }),
        { signal },
      ),
  });

/** Starts the setup: a secret for an authenticator app, as text and as an address it can open. */
export function setupTwoFactor() {
  return apiRequest('/auth/2fa/setup', z.object({ secret: z.string(), uri: z.string() }), {
    method: 'POST',
  });
}

/** Turns two-factor sign-in on with the app's first code; the recovery codes come back once. */
export function enableTwoFactor(code: string) {
  return apiRequest('/auth/2fa/enable', z.object({ recovery_codes: z.array(z.string()) }), {
    method: 'POST',
    body: { code },
  });
}

export function disableTwoFactor(password: string, code: string) {
  return apiSend('/auth/2fa/disable', { method: 'POST', body: { password, code } });
}

/** Signs out everywhere, this device included. */
export function endSessions() {
  return apiSend('/auth/sessions/end', { method: 'POST' });
}

/** Downloads a copy of what the installation holds about the signed-in person. */
export function exportAccount() {
  return saveDownload('/auth/me/export', 'nexc-account.json');
}

/** Deletes the signed-in person's account; their password confirms it. */
export function deleteAccount(password: string) {
  return apiSend('/auth/me/delete', { method: 'POST', body: { password } });
}
