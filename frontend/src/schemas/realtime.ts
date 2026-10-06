import { z } from 'zod';

import { idSchema } from './common';
import {
  edgeSuggestionSchema,
  graphEdgeSchema,
  graphNodeSchema,
  graphSummarySchema,
  nodeStatusSchema,
  ontologySchema,
} from './graph';
import { planSchema, proposedEdgeSchema, proposedNodeSchema } from './plan';
import { artifactSchema, runSchema } from './run';

export const realtimeTicketSchema = z.object({
  ticket: z.string().min(1),
  expires_in: z.number().int().positive(),
});
export type RealtimeTicket = z.infer<typeof realtimeTicketSchema>;

/* ---------- SSE (CONTRACT §6): event name → data payload ---------- */

export const sseEventSchemas = {
  'plan.started': z.object({ plan_id: idSchema }),
  'plan.node': z.object({ plan_id: idSchema, node: proposedNodeSchema }),
  'plan.edge': z.object({ plan_id: idSchema, edge: proposedEdgeSchema }),
  'plan.ready': z.object({ plan: planSchema }),
  'plan.failed': z.object({ plan_id: idSchema, error: z.string() }),
  'run.started': z.object({ run: runSchema }),
  'node.status': z.object({
    run_id: idSchema,
    node_id: idSchema,
    status: nodeStatusSchema,
    attempt: z.number().int().nonnegative(),
    error: z.string().nullable(),
    cached: z.boolean(),
  }),
  'node.output': z.object({ run_id: idSchema, node_id: idSchema, delta: z.string() }),
  'node.log': z.object({
    run_id: idSchema,
    node_id: idSchema,
    level: z.string(),
    message: z.string(),
  }),
  'node.tokens': z.object({
    run_id: idSchema,
    node_id: idSchema,
    tokens_in: z.number().int().nonnegative(),
    tokens_out: z.number().int().nonnegative(),
  }),
  'artifact.created': z.object({ artifact: artifactSchema }),
  'run.finished': z.object({ run: runSchema }),
  heartbeat: z.object({ at: z.string() }),
} as const;

export type SseEventType = keyof typeof sseEventSchemas;
export const SSE_EVENT_TYPES = Object.keys(sseEventSchemas) as SseEventType[];

/** Discriminated union `{ type, data }` over every SSE event. */
export type SseEvent = {
  [K in SseEventType]: { type: K; id: string | null; data: z.infer<(typeof sseEventSchemas)[K]> };
}[SseEventType];

export type SseEventOf<K extends SseEventType> = Extract<SseEvent, { type: K }>;

/* ---------- WebSocket (CONTRACT §7) ---------- */

const cursorSchema = z.object({ x: z.number(), y: z.number() }).nullable();
export type Cursor = z.infer<typeof cursorSchema>;

export const wsServerMessageSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('node.upserted'), node: graphNodeSchema }),
  z.object({ type: z.literal('node.deleted'), node_id: idSchema }),
  z.object({ type: z.literal('edge.upserted'), edge: graphEdgeSchema }),
  z.object({ type: z.literal('edge.deleted'), edge_id: idSchema }),
  z.object({ type: z.literal('graph.updated'), graph: graphSummarySchema }),
  z.object({ type: z.literal('ontology.updated'), ontology: ontologySchema }),
  z.object({ type: z.literal('suggestions'), items: z.array(edgeSuggestionSchema) }),
  z.object({
    type: z.literal('presence'),
    user_id: idSchema,
    name: z.string(),
    cursor: cursorSchema,
  }),
  z.object({ type: z.literal('pong') }),
]);
export type WsServerMessage = z.infer<typeof wsServerMessageSchema>;

export type WsClientMessage =
  | { type: 'node.move'; node_id: string; x: number; y: number }
  | { type: 'presence'; cursor: Cursor }
  | { type: 'ping' };
