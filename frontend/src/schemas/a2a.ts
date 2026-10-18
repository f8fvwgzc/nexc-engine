import { z } from 'zod';

import { dataLabelSchema, issuedAgentApiTokenSchema } from './agent';
import { idSchema, nullableTimestampSchema, timestampSchema } from './common';

/* ---------- A2A counterparts (CONTRACT §4, §10) ---------- */

/** A remote A2A agent the workspace registered; the header it is sent never shown. */
export const a2aCounterpartSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  /** The tool agents call it by is `a2a:<name>`. */
  name: z.string(),
  card_url: z.string(),
  host: z.string(),
  /** The agent card as last fetched; `{}` until one was. */
  card: z.record(z.string(), z.unknown()),
  has_auth_header: z.boolean(),
  /** Local agents that may call it; empty means any agent the policy allows. */
  allowed_agents: z.array(idSchema),
  max_label: dataLabelSchema,
  /** The external agent principal its inbound calls act as. */
  agent_id: idSchema,
  principal_id: z.string(),
  last_checked_at: nullableTimestampSchema,
  last_check_ok: z.boolean().nullable(),
  last_check_detail: z.string(),
  created_by: idSchema.nullable(),
  created_at: timestampSchema,
  updated_at: timestampSchema,
});
export type A2aCounterpart = z.infer<typeof a2aCounterpartSchema>;

/** The answer to a registration: the counterpart and its principal's first token, shown once. */
export const registeredA2aCounterpartSchema = z.object({
  counterpart: a2aCounterpartSchema,
  token: issuedAgentApiTokenSchema,
});
export type RegisteredA2aCounterpart = z.infer<typeof registeredA2aCounterpartSchema>;

/** What fetching the card found. */
export const a2aCheckSchema = z.object({
  ok: z.boolean(),
  detail: z.string(),
  card: z.record(z.string(), z.unknown()).nullable(),
  latency_ms: z.number(),
});
export type A2aCheck = z.infer<typeof a2aCheckSchema>;

/** What a counterpart is registered with. */
export const a2aCounterpartInputSchema = z.object({
  name: z
    .string()
    .trim()
    .min(1, { error: 'Name is required' })
    .max(64)
    .regex(/^[a-z0-9_-]+$/, { error: 'Use lowercase letters, digits, - or _' }),
  card_url: z
    .string()
    .trim()
    .min(1, { error: 'The card URL is required' })
    .max(2000)
    .regex(/^https?:\/\//, { error: 'Use an http(s) URL' }),
  /** Kept encrypted; never shown again. */
  auth_header: z.string().max(2000),
  allowed_agents: z.array(idSchema).max(64),
  max_label: dataLabelSchema,
});
export type A2aCounterpartInput = z.infer<typeof a2aCounterpartInputSchema>;

export const EMPTY_A2A_COUNTERPART: A2aCounterpartInput = {
  name: '',
  card_url: '',
  auth_header: '',
  allowed_agents: [],
  max_label: 'internal',
};

/** The request body of `POST /workspaces/{wid}/a2a-counterparts`. */
export function toA2aCounterpartBody(input: A2aCounterpartInput) {
  return {
    name: input.name,
    card_url: input.card_url,
    ...(input.auth_header ? { auth_header: input.auth_header } : {}),
    allowed_agents: input.allowed_agents,
    max_label: input.max_label,
  };
}

/** What the card says the remote agent is called, when one was fetched. */
export function cardName(counterpart: A2aCounterpart): string | null {
  const name = counterpart.card.name;
  return typeof name === 'string' && name ? name : null;
}

/** Whether the card says the remote agent streams (`tasks/sendSubscribe`). */
export function cardStreams(counterpart: A2aCounterpart): boolean {
  const capabilities = counterpart.card.capabilities;
  return (
    typeof capabilities === 'object' &&
    capabilities !== null &&
    (capabilities as Record<string, unknown>).streaming === true
  );
}
