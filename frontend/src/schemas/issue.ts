import { z } from 'zod';

import { idSchema, timestampSchema } from './common';

export const stateCategorySchema = z.enum([
  'backlog',
  'unstarted',
  'started',
  'completed',
  'canceled',
]);
export type StateCategory = z.infer<typeof stateCategorySchema>;

/** One state of a team's workflow. Names and colours are the team's; the category is fixed. */
export const issueStateSchema = z.object({
  id: idSchema,
  team_id: idSchema,
  name: z.string(),
  category: stateCategorySchema,
  color: z.string(),
  position: z.number().int(),
});
export type IssueState = z.infer<typeof issueStateSchema>;

export const issueSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  team_id: idSchema,
  /** Team key and number, e.g. ENG-12. */
  identifier: z.string(),
  number: z.number().int(),
  title: z.string(),
  description: z.string(),
  state: issueStateSchema,
  /** 0 none, 1 urgent, 2 high, 3 medium, 4 low. */
  priority: z.number().int().min(0).max(4),
  assignee: z.object({ user_id: idSchema, name: z.string() }).nullable(),
  agent_id: idSchema.nullable(),
  project_id: idSchema.nullable(),
  /** The graph that plans and executes the issue, if one was created. */
  graph_id: idSchema.nullable(),
  creator_id: idSchema.nullable(),
  created_at: timestampSchema,
  updated_at: timestampSchema,
  completed_at: timestampSchema.nullable(),
});
export type Issue = z.infer<typeof issueSchema>;

export const projectStatusSchema = z.enum([
  'planned',
  'started',
  'paused',
  'completed',
  'canceled',
]);
export type ProjectStatus = z.infer<typeof projectStatusSchema>;

export const projectSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  name: z.string(),
  description: z.string(),
  status: projectStatusSchema,
  lead_id: idSchema.nullable(),
  target_date: z.string().nullable(),
  issue_count: z.number().int().nonnegative(),
  closed_count: z.number().int().nonnegative(),
  created_at: timestampSchema,
});
export type Project = z.infer<typeof projectSchema>;

export const PRIORITY_LABEL = ['No priority', 'Urgent', 'High', 'Medium', 'Low'] as const;

export interface IssueInput {
  title: string;
  description?: string;
  state_id?: string;
  priority?: number;
  assignee_id?: string | null;
  agent_id?: string | null;
  project_id?: string | null;
}
