import { z } from 'zod';

import { timestampSchema } from './common';

/** How the backend reaches onto-intelli, where the workspace's ontology lives. */
export const ontoModeSchema = z.enum(['service', 'embedded', 'off']);
export type OntoMode = z.infer<typeof ontoModeSchema>;

export const proposalStatusSchema = z.enum(['open', 'applied', 'rejected', 'failed', 'withdrawn']);

/** A schema change proposal in onto-intelli; people review it there. */
export const schemaProposalSchema = z.object({
  id: z.string(),
  title: z.string(),
  rationale: z.string(),
  status: proposalStatusSchema,
  /** `user:{id}`, or `assistant:{user id}` when the assistant filed it. */
  proposer: z.string(),
  by_agent: z.boolean(),
  breaking: z.boolean(),
  changes: z.number().int().nonnegative(),
  created_at: timestampSchema,
  review_url: z.string().nullable(),
});
export type SchemaProposal = z.infer<typeof schemaProposalSchema>;

/** The workspace's ontology: where it is kept and the proposals still open. */
export const ontologyStatusSchema = z.object({
  mode: ontoModeSchema,
  ui_url: z.string().nullable(),
  tenant: z.string().nullable(),
  workspace: z.string().nullable(),
  proposals: z.array(schemaProposalSchema),
  error: z.string().nullable(),
});
export type OntologyStatus = z.infer<typeof ontologyStatusSchema>;

/** The member's own short-lived session for onto-intelli's UI, handed to it by postMessage. */
export const ontologySessionSchema = z.object({
  ui_url: z.string(),
  token: z.string(),
  tenant: z.string(),
  workspace: z.string(),
  expires_at: timestampSchema,
});
export type OntologySession = z.infer<typeof ontologySessionSchema>;

/**
 * The views of onto-intelli's UI the console opens: the map (optionally focused on an entity),
 * one entity, the schema editor and the proposals inbox. Anything else is not opened.
 */
export function ontoView(raw: string | null): string {
  const view = raw?.trim() ?? '';
  return /^\/(map|entities|schema|proposals)(\/[\w.:@%-]+)*(\?[\w=&.:%-]*)?$/.test(view)
    ? view
    : '/map';
}

/** The onto-intelli entity of something nexc shows (`issue:…`, `run:…`, `agent:…`). */
export function ontoEntity(kind: 'issue' | 'run' | 'agent' | 'graph', key: string): string {
  if (kind === 'graph') return `execution_graph:${key}`;
  if (kind === 'agent') return `agent:${key.replace(/^agent:\/\//, '')}`;
  return `${kind}:${key}`;
}

/** Where the console opens one onto-intelli entity: its page in the embedded UI. */
export function entityPath(entity: string): string {
  return `/app/settings/map?view=${encodeURIComponent(`/entities/${encodeURIComponent(entity)}`)}`;
}
