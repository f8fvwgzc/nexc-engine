import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { saveDownload } from '@/lib/api/download';
import { authResponseSchema, userSchema } from '@/schemas/auth';

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
