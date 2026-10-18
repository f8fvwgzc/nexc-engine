import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiFetch, apiRequest } from '@/lib/api/client';
import { ContractError } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import {
  AUDIT_QUESTIONS,
  auditAnswerSchema,
  cannedQuestionSchema,
  type AuditAnswer,
} from '@/schemas/audit';

import { SseFrameParser } from './sse-frames';

/** The canned questions, as the backend offers them. */
export const auditQuestionsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.auditQuestions(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/audit/questions`, z.array(cannedQuestionSchema), {
        signal,
      }),
    placeholderData: AUDIT_QUESTIONS,
    staleTime: 600_000,
  });

/** Asks the audit and waits for the whole answer. */
export function askAudit(workspaceId: string, question: string, signal?: AbortSignal) {
  return apiRequest(`/workspaces/${workspaceId}/audit/ask`, auditAnswerSchema, {
    method: 'POST',
    body: { question },
    signal,
  });
}

export type AuditStreamEvent =
  | { type: 'stage'; text: string }
  | { type: 'delta'; text: string }
  | { type: 'done'; answer: AuditAnswer }
  | { type: 'error'; detail: string };

const textFrame = z.object({ text: z.string() });
const errorFrame = z.object({ detail: z.string() });

/**
 * Asks the audit and reports the answer as it is produced: stages and text while a model
 * explains, then `done` with the whole answer (which is also returned), or `error`.
 */
export async function askAuditStream(
  workspaceId: string,
  question: string,
  onEvent: (event: AuditStreamEvent) => void,
  signal?: AbortSignal,
): Promise<AuditAnswer> {
  const path = `/workspaces/${workspaceId}/audit/ask/stream`;
  const res = await apiFetch(path, {
    method: 'POST',
    body: { question },
    headers: { Accept: 'text/event-stream' },
    signal,
  });
  if (!res.body) throw new ContractError(path, []);
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  const parser = new SseFrameParser();
  let answer: AuditAnswer | null = null;
  let failure: string | null = null;
  for (;;) {
    const { done, value } = await reader.read();
    const chunk = done ? decoder.decode() : decoder.decode(value, { stream: true });
    for (const frame of parser.push(chunk + (done ? '\n\n' : ''))) {
      let json: unknown;
      try {
        json = JSON.parse(frame.data);
      } catch {
        continue;
      }
      switch (frame.event) {
        case 'stage':
        case 'delta': {
          const parsed = textFrame.safeParse(json);
          if (parsed.success) onEvent({ type: frame.event, text: parsed.data.text });
          break;
        }
        case 'done': {
          const parsed = auditAnswerSchema.safeParse(json);
          if (!parsed.success) throw new ContractError(path, parsed.error.issues);
          answer = parsed.data;
          onEvent({ type: 'done', answer });
          break;
        }
        case 'error': {
          const parsed = errorFrame.safeParse(json);
          failure = parsed.success ? parsed.data.detail : 'The audit could not answer.';
          onEvent({ type: 'error', detail: failure });
          break;
        }
        default:
          break;
      }
    }
    if (done || answer || failure) break;
  }
  if (answer) return answer;
  throw new Error(failure ?? 'The audit ended its answer early.');
}
