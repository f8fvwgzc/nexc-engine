import { z } from 'zod';

import { idSchema, timestampSchema } from './common';

/** A team's cycle; `starts_on` and `ends_on` are `YYYY-MM-DD` days, both included. */
export const cycleSchema = z.object({
  id: idSchema,
  team_id: idSchema,
  number: z.number().int(),
  name: z.string(),
  starts_on: z.string(),
  ends_on: z.string(),
  status: z.enum(['upcoming', 'active', 'completed']),
  issue_count: z.number().int().nonnegative(),
  closed_count: z.number().int().nonnegative(),
  created_at: timestampSchema,
});
export type Cycle = z.infer<typeof cycleSchema>;

/** A cycle is known by its name, or by its number when it has none. */
export function cycleLabel(cycle: { number: number; name: string }): string {
  return cycle.name ? `${cycle.name} (#${cycle.number})` : `Cycle ${cycle.number}`;
}
