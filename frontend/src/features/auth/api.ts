import { queryOptions } from '@tanstack/react-query';

import { apiRequest, apiSend } from '@/lib/api/client';
import { CSRF_HEADERS } from '@/lib/api/session';
import { qk } from '@/lib/query-keys';
import {
  authResponseSchema,
  userSchema,
  type LoginInput,
  type RegisterInput,
} from '@/schemas/auth';

export function login(body: LoginInput) {
  return apiRequest('/auth/login', authResponseSchema, { method: 'POST', body, auth: false });
}

export function register(body: RegisterInput) {
  return apiRequest('/auth/register', authResponseSchema, { method: 'POST', body, auth: false });
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
