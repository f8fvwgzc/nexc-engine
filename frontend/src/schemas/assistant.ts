import { z } from 'zod';

import { issueSchema } from './issue';

export const assistantReplySchema = z.object({
  reply: z.string(),
  /** Issues the assistant filed while answering. */
  created: z.array(issueSchema),
  /** Drafts it could not file. */
  skipped: z.array(z.string()),
  memories_used: z.number().int().nonnegative(),
});
export type AssistantReply = z.infer<typeof assistantReplySchema>;

export interface AssistantTurn {
  role: 'user' | 'assistant';
  content: string;
}
