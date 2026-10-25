import { z } from 'zod';

/** A public page a skill was written from. */
export const skillSourceSchema = z.object({
  title: z.string(),
  url: z.string(),
});
export type SkillSource = z.infer<typeof skillSourceSchema>;

/** A skill as `GET /skills` lists it: what it knows, without the knowledge itself. */
export const skillSummarySchema = z.object({
  /** Its id in agents' skill lists (`fin-equity-valuation`). */
  id: z.string().min(1),
  name: z.string(),
  /** The field it belongs to (`finance`, `shipping`, ...). */
  category: z.string(),
  description: z.string(),
  /** Terms a step that needs it tends to use: they rank skills for a step. */
  keywords: z.array(z.string()),
  sources: z.array(skillSourceSchema),
  /** Characters of its body: what it costs in a prompt. */
  chars: z.number().int().nonnegative(),
});
export type SkillSummary = z.infer<typeof skillSummarySchema>;

/** A skill with its working knowledge, in Markdown (`GET /skills/{id}`). */
export const skillSchema = skillSummarySchema.omit({ chars: true }).extend({
  body: z.string(),
});
export type Skill = z.infer<typeof skillSchema>;
