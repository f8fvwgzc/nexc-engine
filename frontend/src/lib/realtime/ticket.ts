import { apiRequest } from '@/lib/api/client';
import { realtimeTicketSchema, type RealtimeTicket } from '@/schemas/realtime';

/** Single-use, 30 s ticket that authenticates one SSE or WebSocket connection (CONTRACT §5). */
export function requestRealtimeTicket(
  graphId: string,
  signal?: AbortSignal,
): Promise<RealtimeTicket> {
  return apiRequest('/realtime/tickets', realtimeTicketSchema, {
    method: 'POST',
    body: { graph_id: graphId },
    signal,
  });
}
