import { z } from 'zod';

import { idSchema, nullableTimestampSchema, timestampSchema } from './common';

export const agentStatusSchema = z.enum(['active', 'paused', 'over_budget']);
export type AgentStatus = z.infer<typeof agentStatusSchema>;

export const agentRuntimeSchema = z.enum(['python', 'builtin']);
export type AgentRuntime = z.infer<typeof agentRuntimeSchema>;

export const agentSchema = z.object({
  id: idSchema,
  name: z.string(),
  role: z.string(),
  title: z.string(),
  model: z.string(),
  system_prompt: z.string(),
  reports_to: idSchema.nullable(),
  budget_tokens: z.number().int().nonnegative(),
  spent_tokens: z.number().int().nonnegative(),
  status: agentStatusSchema,
  runtime: agentRuntimeSchema,
  heartbeat_at: nullableTimestampSchema,
  created_at: timestampSchema,
});
export type Agent = z.infer<typeof agentSchema>;

export const agentInputSchema = z.object({
  name: z.string().trim().min(1, { error: 'Name is required' }).max(100),
  role: z
    .string()
    .trim()
    .min(1, { error: 'Role is required' })
    .max(100)
    .regex(/^[a-z0-9_-]+$/, { error: 'Use lowercase letters, digits, - or _' }),
  title: z.string().trim().max(200),
  model: z.string().trim().max(200),
  system_prompt: z.string().max(64 * 1024),
  reports_to: idSchema.nullable(),
  budget_tokens: z
    .number({ error: 'Enter a number' })
    .int({ error: 'Use a whole number' })
    .nonnegative({ error: 'Budget cannot be negative' }),
  runtime: agentRuntimeSchema,
});
export type AgentInput = z.infer<typeof agentInputSchema>;
