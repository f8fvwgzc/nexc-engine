import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import {
  a2aCheckSchema,
  a2aCounterpartSchema,
  registeredA2aCounterpartSchema,
  toA2aCounterpartBody,
  type A2aCounterpartInput,
} from '@/schemas/a2a';

/** The A2A counterparts of a workspace, by name. */
export const a2aCounterpartsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.a2a.list(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/a2a-counterparts`, z.array(a2aCounterpartSchema), {
        signal,
      }),
  });

/** Registers a counterpart (admins): its card is fetched, its principal made, a token issued once. */
export function registerA2aCounterpart(workspaceId: string, input: A2aCounterpartInput) {
  return apiRequest(`/workspaces/${workspaceId}/a2a-counterparts`, registeredA2aCounterpartSchema, {
    method: 'POST',
    body: toA2aCounterpartBody(input),
  });
}

/** Fetches the counterpart's card again and records what it says (admins). */
export function checkA2aCounterpart(workspaceId: string, counterpartId: string) {
  return apiRequest(
    `/workspaces/${workspaceId}/a2a-counterparts/${counterpartId}/check`,
    a2aCheckSchema,
    { method: 'POST' },
  );
}

/** Removes a counterpart with its principal (admins). */
export function removeA2aCounterpart(workspaceId: string, counterpartId: string) {
  return apiSend(`/workspaces/${workspaceId}/a2a-counterparts/${counterpartId}`, {
    method: 'DELETE',
  });
}

/** Where an exposed agent's card and endpoint are, under the console's origin (CONTRACT §10). */
export function a2aAddresses(principal: string, origin = window.location.origin) {
  const encoded = encodeURIComponent(principal);
  return {
    card: `${origin}/a2a/agents/${encoded}/card`,
    wellKnown: `${origin}/.well-known/agent.json?agent=${encoded}`,
    endpoint: `${origin}/a2a/agents/${encoded}`,
  };
}
