import { z } from 'zod';

import { idSchema, timestampSchema } from './common';
import { issueSchema } from './issue';

/** Where in the app the member is while they write; the assistant is told. */
export const pageContextSchema = z.object({
  path: z.string(),
  title: z.string(),
});
export type PageContext = z.infer<typeof pageContextSchema>;

export const assistantReplySchema = z.object({
  /** The conversation this exchange was saved to; sent back to continue it. */
  conversation_id: idSchema,
  reply: z.string(),
  /** Issues the assistant filed while answering. */
  created: z.array(issueSchema),
  /** Drafts it could not file. */
  skipped: z.array(z.string()),
  memories_used: z.number().int().nonnegative(),
});
export type AssistantReply = z.infer<typeof assistantReplySchema>;

export const assistantRoleSchema = z.enum(['user', 'assistant']);
export type AssistantRole = z.infer<typeof assistantRoleSchema>;

/** An issue the assistant filed, as remembered with the conversation. */
export const filedIssueSchema = z.object({
  id: idSchema,
  identifier: z.string(),
  title: z.string(),
});

/** What an assistant turn did besides answering. */
export const turnOutcomeSchema = z.object({
  created: z.array(filedIssueSchema),
  skipped: z.array(z.string()),
  memories_used: z.number().int().nonnegative(),
});
export type TurnOutcome = z.infer<typeof turnOutcomeSchema>;

/** A saved conversation with the assistant: the member's own, in one workspace. */
export const conversationSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  title: z.string(),
  page_path: z.string(),
  page_title: z.string(),
  message_count: z.number().int().nonnegative(),
  created_at: timestampSchema,
  updated_at: timestampSchema,
});
export type Conversation = z.infer<typeof conversationSchema>;

export const conversationMessageSchema = z.object({
  id: idSchema,
  role: assistantRoleSchema,
  content: z.string(),
  outcome: turnOutcomeSchema.nullable(),
  page_path: z.string(),
  page_title: z.string(),
  created_at: timestampSchema,
});
export type ConversationMessage = z.infer<typeof conversationMessageSchema>;

export const conversationDetailSchema = z.object({
  conversation: conversationSchema,
  messages: z.array(conversationMessageSchema),
});
export type ConversationDetail = z.infer<typeof conversationDetailSchema>;
