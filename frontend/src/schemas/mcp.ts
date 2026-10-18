import { z } from 'zod';

import { dataLabelSchema } from './agent';
import { idSchema, nullableTimestampSchema, timestampSchema } from './common';

export const MCP_TRANSPORTS = ['stdio', 'http'] as const;
export const mcpTransportSchema = z.enum(MCP_TRANSPORTS);
export type McpTransport = z.infer<typeof mcpTransportSchema>;

/** A tool as the server lists it; `input_schema` is JSON Schema. */
export const mcpToolSchema = z.object({
  name: z.string(),
  description: z.string(),
  input_schema: z.record(z.string(), z.unknown()),
});
export type McpTool = z.infer<typeof mcpToolSchema>;

/** An MCP server of the workspace, called through the tools gateway (CONTRACT §4). */
export const mcpServerSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  name: z.string(),
  transport: mcpTransportSchema,
  command: z.string().nullable(),
  args: z.array(z.string()),
  url: z.string().nullable(),
  has_auth_header: z.boolean(),
  /** Empty means every tool the server lists. */
  allowed_tools: z.array(z.string()),
  /** The strictest classification of data that may be sent to the server. */
  max_label: dataLabelSchema,
  tools: z.array(mcpToolSchema),
  last_checked_at: nullableTimestampSchema,
  last_check_ok: z.boolean().nullable(),
  last_check_detail: z.string(),
  created_by: idSchema.nullable(),
  created_at: timestampSchema,
  updated_at: timestampSchema,
});
export type McpServer = z.infer<typeof mcpServerSchema>;

export const mcpCheckSchema = z.object({
  ok: z.boolean(),
  detail: z.string(),
  tools: z.array(mcpToolSchema),
  latency_ms: z.number().int(),
});
export type McpCheck = z.infer<typeof mcpCheckSchema>;

/** What a server is registered with. */
export const mcpServerInputSchema = z.object({
  name: z
    .string()
    .trim()
    .min(1, { error: 'Name is required' })
    .max(64)
    .regex(/^[a-z0-9_-]+$/, { error: 'Use lowercase letters, digits, - or _' }),
  transport: mcpTransportSchema,
  command: z.string().trim().max(2000),
  /** One argument per line. */
  args: z.string().max(8000),
  url: z.string().trim().max(2000),
  /** Kept encrypted; never shown again. */
  auth_header: z.string().max(2000),
  /** Tool names, one per line; empty allows every tool. */
  allowed_tools: z.string().max(8000),
  max_label: dataLabelSchema,
});
export type McpServerInput = z.infer<typeof mcpServerInputSchema>;

export const EMPTY_MCP_SERVER: McpServerInput = {
  name: '',
  transport: 'stdio',
  command: '',
  args: '',
  url: '',
  auth_header: '',
  allowed_tools: '',
  max_label: 'internal',
};

/** The request body of `POST /workspaces/{wid}/mcp-servers`. */
export function toMcpServerBody(input: McpServerInput) {
  const lines = (text: string) =>
    text
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean);
  const stdio = input.transport === 'stdio';
  return {
    name: input.name,
    transport: input.transport,
    ...(stdio ? { command: input.command, args: lines(input.args) } : { url: input.url }),
    ...(!stdio && input.auth_header ? { auth_header: input.auth_header } : {}),
    allowed_tools: lines(input.allowed_tools),
    max_label: input.max_label,
  };
}
