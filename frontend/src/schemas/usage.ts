import { z } from 'zod';

import { idSchema } from './common';

const totals = {
  calls: z.number().int().nonnegative(),
  tokens_in: z.number().int().nonnegative(),
  tokens_out: z.number().int().nonnegative(),
  cost_usd: z.number().nonnegative(),
  /** Characters of upstream context left out of prompts as padding or irrelevant. */
  context_chars_saved: z.number().int().nonnegative(),
};
export const usageTotalsSchema = z.object(totals);
export type UsageTotals = z.infer<typeof usageTotalsSchema>;

/** Totals of one group; the group is described by `key`. */
const slice = <K extends z.ZodType>(key: K) => z.object({ key, ...totals });

/** Token usage of a workspace over the last `days` days. */
export const usageReportSchema = z.object({
  /** `workspace` for admins; other members get a report of their own usage. */
  scope: z.enum(['workspace', 'own']),
  days: z.number().int().positive(),
  totals: usageTotalsSchema,
  by_day: z.array(slice(z.string())),
  by_member: z.array(slice(z.object({ user_id: idSchema.nullable(), name: z.string() }))),
  by_model: z.array(slice(z.object({ provider: z.string(), model: z.string() }))),
  by_purpose: z.array(slice(z.enum(['plan', 'node', 'memory']))),
  /** Whose account paid: members' own, the workspace's, or the server's. */
  by_credential: z.array(slice(z.enum(['user', 'workspace', 'server']))),
});
export type UsageReport = z.infer<typeof usageReportSchema>;
