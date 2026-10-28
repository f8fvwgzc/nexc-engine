import { z } from 'zod';

import { idSchema, timestampSchema } from './common';
import { alertKindSchema, approvalStatusSchema, severitySchema } from './policy';
import { runStatusSchema } from './run';

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

/** A label of the workspace, as carried by an issue. */
export const labelSchema = z.object({ id: idSchema, name: z.string(), color: z.string() });
export type Label = z.infer<typeof labelSchema>;

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
  labels: z.array(labelSchema),
  /** The issue this one is part of, if the caller can see it. */
  parent: z.object({ id: idSchema, identifier: z.string(), title: z.string() }).nullable(),
  /** The cycle of its team the issue is planned in. */
  cycle: z.object({ id: idSchema, number: z.number().int(), name: z.string() }).nullable(),
  sub_issues: z.object({
    total: z.number().int().nonnegative(),
    closed: z.number().int().nonnegative(),
  }),
  assignee: z.object({ user_id: idSchema, name: z.string() }).nullable(),
  agent_id: idSchema.nullable(),
  project_id: idSchema.nullable(),
  /** The graph that plans and executes the issue, if one was created. */
  graph_id: idSchema.nullable(),
  creator_id: idSchema.nullable(),
  /** The calendar day the issue is due (`YYYY-MM-DD`). */
  due_date: z.string().nullable(),
  /** The GitHub issue or pull request the issue is linked to (a link only; nothing syncs). */
  github_url: z.string().nullable(),
  created_at: timestampSchema,
  updated_at: timestampSchema,
  completed_at: timestampSchema.nullable(),
});
export type Issue = z.infer<typeof issueSchema>;

/**
 * An issue as lists carry it (`GET /workspaces/{wid}/issues`, search): every field but the
 * description, which only `GET /issues/{iid}` returns, plus the status of the latest run of the
 * issue's graph (null without a graph or runs).
 */
export const issueSummarySchema = issueSchema.omit({ description: true }).extend({
  run_status: runStatusSchema.nullable(),
});
export type IssueSummary = z.infer<typeof issueSummarySchema>;

/** `GET /workspaces/{wid}/inbox/unread-count`: unread notifications, at most 1,000. */
export const unreadCountSchema = z.object({ count: z.number().int().nonnegative() });

export const issueEventKindSchema = z.enum([
  'comment',
  'state',
  'priority',
  'assignee',
  'title',
  'due',
  /** A run of the issue's graph finished; `to` is its status, `body` its result. */
  'run',
  /** The GitHub link changed; `from` and `to` are the old and new URLs. */
  'github',
  /**
   * A file of the issue changed; `body` is its name, `to` what happened (`uploaded`,
   * `updated:v<N>`, `duplicated`, `deleted`).
   */
  'artifact',
  /**
   * A comment that mentioned an agent handed it the issue; `to` is the agent's name, `body` what
   * was asked (`delegated`, `asked to plan`).
   */
  'agent',
]);

/** The emoji a comment or a run report can be reacted to with (the server's allow-list). */
export const REACTION_EMOJI = ['👍', '👎', '😄', '🎉', '😕', '❤️', '🚀', '👀', '🔥', '✅'] as const;

/** The reactions with one emoji on an entry. */
export const reactionSchema = z.object({
  emoji: z.string(),
  count: z.number().int().nonnegative(),
  /** Whether the caller is one of them. */
  reacted: z.boolean(),
  /** Who reacted, newest first; at most ten of them. */
  users: z.array(z.object({ user_id: idSchema, name: z.string() })),
});
export type Reaction = z.infer<typeof reactionSchema>;

/** One entry of an issue's timeline: a comment, or a change to the issue. */
export const issueEventSchema = z.object({
  id: idSchema,
  issue_id: idSchema,
  kind: issueEventKindSchema,
  actor: z.object({ user_id: idSchema, name: z.string() }).nullable(),
  /** The text of a comment; empty for changes. */
  body: z.string(),
  from: z.string().nullable(),
  to: z.string().nullable(),
  created_at: timestampSchema,
  edited_at: timestampSchema.nullable(),
  /** The run a `run` entry reports; null for other entries and once the run is deleted. */
  run_id: idSchema.nullable(),
  /** The reactions to a comment or a run report, by emoji; none on other entries. */
  reactions: z.array(reactionSchema).default(() => []),
  /** The comment a reply answers; null for a comment of its own and for every other entry. */
  parent_id: idSchema.nullable().default(null),
});
export type IssueEvent = z.infer<typeof issueEventSchema>;

/** One entry of the caller's inbox. */
export const notificationSchema = z.object({
  id: idSchema,
  kind: z.enum([
    'assigned',
    'comment',
    'state',
    'due',
    'approval',
    'alert',
    'run',
    'mention',
    'reaction',
  ]),
  /** The issue; null for approvals and alerts. */
  issue: z.object({ id: idSchema, identifier: z.string(), title: z.string() }).nullable(),
  /** The tool call to decide (`approval`). */
  approval: z
    .object({
      id: idSchema,
      run_id: idSchema,
      node_id: idSchema,
      graph_id: idSchema,
      principal: z.string(),
      tool: z.string(),
      reason: z.string(),
      status: approvalStatusSchema,
      expires_at: timestampSchema,
    })
    .nullable(),
  /** The alert to look at (`alert`). */
  alert: z
    .object({
      id: idSchema,
      kind: alertKindSchema,
      severity: severitySchema,
      rule: z.string(),
      title: z.string(),
      principal: z.string(),
      run_id: idSchema.nullable(),
      graph_id: idSchema.nullable(),
    })
    .nullable(),
  actor: z.object({ user_id: idSchema, name: z.string() }).nullable(),
  /** What the actor reacted to the caller's comment with (`reaction`). */
  emoji: z.string().nullable().default(null),
  created_at: timestampSchema,
  read_at: timestampSchema.nullable(),
});
export type Notification = z.infer<typeof notificationSchema>;

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
  /** The team that owns the project; null for a project of the whole workspace. */
  team_id: idSchema.nullable(),
  /** That team, as much as a chip shows. */
  team: z.object({ id: idSchema, key: z.string(), name: z.string() }).nullable(),
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
  label_ids?: string[];
  parent_id?: string | null;
  cycle_id?: string | null;
  /** `YYYY-MM-DD`; `null` removes it. */
  due_date?: string | null;
  /** A GitHub issue or pull request URL; `null` or `''` removes the link. */
  github_url?: string | null;
}
