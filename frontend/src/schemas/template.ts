import { z } from 'zod';

/** Built-in starter graph. Template ids are slugs (e.g. `research-report-docx`), not UUIDs. */
export const graphTemplateSchema = z.object({
  id: z.string().min(1),
  name: z.string(),
  description: z.string(),
  category: z.string(),
  node_count: z.number().int().nonnegative(),
  tags: z.array(z.string()),
});
export type GraphTemplate = z.infer<typeof graphTemplateSchema>;

export interface CreateFromTemplateBody {
  template_id: string;
  name?: string;
  /** What this instance is about (research question, product, …); it leads the graph goal. */
  topic?: string;
  /** Workspace to create the graph in (default: the caller's first workspace). */
  workspace_id?: string;
}
