import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import {
  alertDeliverySchema,
  alertDestinationSchema,
  destinationTestSchema,
  issuedAlertDestinationSchema,
  type AlertDestinationInput,
  type AlertDestinationPatch,
} from '@/schemas/alert-routing';

/** The alert destinations of a workspace, by name (admins). */
export const alertDestinationsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.alertDestinations.list(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/alert-destinations`, z.array(alertDestinationSchema), {
        signal,
      }),
  });

/** The delivery log of a destination, newest first. */
export const alertDeliveriesQuery = (workspaceId: string, destinationId: string, limit = 50) =>
  queryOptions({
    queryKey: qk.alertDestinations.deliveries(workspaceId, destinationId),
    queryFn: ({ signal }) =>
      apiRequest(
        `/workspaces/${workspaceId}/alert-destinations/${destinationId}/deliveries`,
        z.array(alertDeliverySchema),
        { signal, query: { limit } },
      ),
    refetchInterval: 15_000,
  });

/** Registers a destination (admins); the secret is in the answer once. */
export function registerAlertDestination(workspaceId: string, input: AlertDestinationInput) {
  return apiRequest(`/workspaces/${workspaceId}/alert-destinations`, issuedAlertDestinationSchema, {
    method: 'POST',
    body: input,
  });
}

/** Changes a destination (admins). */
export function updateAlertDestination(
  workspaceId: string,
  destinationId: string,
  patch: AlertDestinationPatch,
) {
  return apiRequest(
    `/workspaces/${workspaceId}/alert-destinations/${destinationId}`,
    alertDestinationSchema,
    { method: 'PATCH', body: patch },
  );
}

/** Removes a destination and its log (admins). */
export function removeAlertDestination(workspaceId: string, destinationId: string) {
  return apiSend(`/workspaces/${workspaceId}/alert-destinations/${destinationId}`, {
    method: 'DELETE',
  });
}

/** Sends a test event at once and says what the destination answered (admins). */
export function testAlertDestination(workspaceId: string, destinationId: string) {
  return apiRequest(
    `/workspaces/${workspaceId}/alert-destinations/${destinationId}/test`,
    destinationTestSchema,
    { method: 'POST' },
  );
}
