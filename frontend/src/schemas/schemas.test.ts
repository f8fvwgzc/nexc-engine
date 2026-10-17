import { describe, expect, it } from 'vitest';
import type { z } from 'zod';

import * as f from '@/test/fixtures';

import { agentCardSchema, agentSchema } from './agent';
import { authResponseSchema, registerInputSchema, userSchema } from './auth';
import { graphSchema, graphSummarySchema, nodeFormSchema, parseTags } from './graph';
import { memorySchema } from './memory';
import { orchestratorStatusSchema } from './orchestrator';
import { planSchema } from './plan';
import { problemSchema } from './problem';
import { wsServerMessageSchema } from './realtime';
import { artifactSchema, runSchema } from './run';
import { llmSettingsSchema } from './settings';
import { graphTemplateSchema } from './template';
import { spanSchema, tracePageSchema, traceVerificationSchema } from './trace';

describe('contract schemas (CONTRACT §4)', () => {
  it.each([
    ['User', userSchema, f.user],
    ['AuthResponse', authResponseSchema, f.authResponse],
    ['Graph', graphSchema, f.graph],
    ['Plan', planSchema, f.plan],
    ['Run', runSchema, f.run],
    ['Artifact', artifactSchema, f.artifact],
    ['Agent', agentSchema, f.agent],
    ['AgentCard', agentCardSchema, f.agentCard],
    ['Memory', memorySchema, f.memory],
    ['OrchestratorStatus', orchestratorStatusSchema, f.orchestratorStatus],
    ['GraphTemplate', graphTemplateSchema, f.template],
    ['LlmSettings', llmSettingsSchema, f.llmSettings],
    ['Span', spanSchema, f.span],
    ['TracePage', tracePageSchema, { spans: [f.span], next: f.span.id }],
    [
      'TraceVerification',
      traceVerificationSchema,
      {
        ok: false,
        spans: 1,
        hash: f.span.hash,
        sealed_hash: null,
        broken: { span_id: f.span.id, seq: 0, reason: "hash does not match the span's content" },
      },
    ],
  ] as const)('parses a valid %s', (_name, schema: z.ZodType, fixture) => {
    expect(schema.parse(fixture)).toEqual(fixture);
  });

  it('parses a GraphSummary', () => {
    const summary = {
      id: f.ids.graph,
      workspace_id: f.ids.workspace,
      team_id: null,
      name: 'g',
      description: '',
      node_count: 2,
      edge_count: 1,
      updated_at: f.graph.updated_at,
    };
    expect(graphSummarySchema.parse(summary)).toEqual(summary);
  });

  it('rejects statuses outside the contract (e.g. "cached" is a flag, not a NodeStatus)', () => {
    expect(
      graphSchema.safeParse({ ...f.graph, nodes: [{ ...f.node, status: 'cached' }] }).success,
    ).toBe(false);
  });

  it('rejects spans of unknown kinds or with malformed hashes', () => {
    expect(spanSchema.safeParse({ ...f.span, kind: 'teleport' }).success).toBe(false);
    expect(spanSchema.safeParse({ ...f.span, hash: 'ABC' }).success).toBe(false);
    expect(spanSchema.safeParse({ ...f.span, parent_id: 'reported-by-the-runtime' }).success).toBe(
      true,
    );
  });

  it('rejects non-UUID ids and missing fields', () => {
    expect(userSchema.safeParse({ ...f.user, id: '42' }).success).toBe(false);
    const { cost_usd: _drop, ...withoutCost } = f.run;
    expect(runSchema.safeParse(withoutCost).success).toBe(false);
  });

  it('accepts template slugs as ids and the demo provider', () => {
    expect(graphTemplateSchema.parse(f.template).id).toBe('research-report-docx');
    expect(llmSettingsSchema.parse(f.llmSettings).provider).toBe('demo');
  });

  it('parses RFC 7807 problems with field errors', () => {
    const problem = problemSchema.parse({
      title: 'Validation failed',
      status: 422,
      detail: '...',
      errors: { email: ['invalid email'] },
    });
    expect(problem.type).toBe('about:blank');
    expect(problem.errors?.email).toEqual(['invalid email']);
  });

  it('parses WebSocket server messages (CONTRACT §7)', () => {
    expect(wsServerMessageSchema.parse({ type: 'node.upserted', node: f.node }).type).toBe(
      'node.upserted',
    );
    expect(
      wsServerMessageSchema.parse({
        type: 'presence',
        user_id: f.ids.user,
        name: 'Ada',
        cursor: null,
      }).type,
    ).toBe('presence');
    expect(wsServerMessageSchema.safeParse({ type: 'node.exploded' }).success).toBe(false);
  });
});

describe('form schemas', () => {
  it('requires a 12+ character password on register', () => {
    const result = registerInputSchema.safeParse({
      name: 'Ada',
      email: 'ada@example.com',
      password: 'short',
    });
    expect(result.success).toBe(false);
    expect(result.error?.issues[0]?.message).toMatch(/at least 12/);
  });

  it('parses comma separated tags without duplicates or blanks', () => {
    expect(parseTags(' a, b ,, a ,c ')).toEqual(['a', 'b', 'c']);
  });

  it('limits node titles to 200 characters', () => {
    const values = {
      title: 'x'.repeat(201),
      content: '',
      kind: 'task',
      executor: 'llm',
      agent_role: '',
      tags: '',
    };
    expect(nodeFormSchema.safeParse(values).success).toBe(false);
  });
});
