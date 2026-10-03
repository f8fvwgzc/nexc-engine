import { z } from 'zod';

export const backendHealthSchema = z.object({
  enabled: z.boolean(),
  ok: z.boolean(),
  url: z.string().nullable(),
  detail: z.string().nullable(),
});
export type BackendHealth = z.infer<typeof backendHealthSchema>;

export const orchestratorStatusSchema = z.object({
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
export type OrchestratorStatus = z.infer<typeof orchestratorStatusSchema>;
