import { z } from 'zod';

/** RFC 7807 problem+json body (CONTRACT §3). Unknown members are tolerated. */
export const problemSchema = z.looseObject({
  type: z.string().default('about:blank'),
  title: z.string(),
  status: z.number().int(),
  detail: z.string().nullish(),
  errors: z.record(z.string(), z.array(z.string())).nullish(),
});
export type Problem = z.infer<typeof problemSchema>;
