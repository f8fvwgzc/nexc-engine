import { z } from 'zod';

export const backendHealthSchema = z.object({
  enabled: z.boolean(),
  ok: z.boolean(),
  url: z.string().nullable(),
  detail: z.string().nullable(),
});
export type BackendHealth = z.infer<typeof backendHealthSchema>;

/** The Planner's live view: its queue, running work, agents and the backends it routes to. */
export const plannerStatusSchema = z.object({
  demo_mode: z.boolean(),
  queue_depth: z.number().int().nonnegative(),
  running_nodes: z.number().int().nonnegative(),
  active_runs: z.number().int().nonnegative(),
  agents_active: z.number().int().nonnegative(),
  backends: z.object({
    llm: backendHealthSchema,
    agent_runtime: backendHealthSchema,
    symphony: backendHealthSchema,
  }),
});
export type PlannerStatus = z.infer<typeof plannerStatusSchema>;
