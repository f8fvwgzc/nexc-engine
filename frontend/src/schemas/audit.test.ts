import { describe, expect, it } from 'vitest';

import { auditEntrySchema } from './audit';

const entry = {
  id: '0199ffff-0000-7000-8000-000000000001',
  actor_id: null,
  actor_name: 'system',
  subject: 'recorded payloads',
  detail: '12 payloads past 30 days',
  created_at: '2026-10-20T09:00:00Z',
};

describe('audit entries', () => {
  // Each one the backend writes (AuditAction in domain/audit.rs): one the log could not read
  // would fail the whole Audit log page.
  it.each([
    'recording_changed',
    'payload_viewed',
    'agent_cancelled',
    'mcp_server_added',
    'mcp_server_removed',
    'agent_token_issued',
    'agent_token_revoked',
    'agent_a2a_changed',
    'a2a_counterpart_added',
    'a2a_counterpart_removed',
    'graph_deleted',
    'evidence_purged',
    'evidence_exported',
    'artifact_file_deleted',
  ])('read %s', (action) => {
    expect(auditEntrySchema.parse({ ...entry, action }).action).toBe(action);
  });

  it('refuse an action the backend does not write', () => {
    expect(auditEntrySchema.safeParse({ ...entry, action: 'graph_renamed' }).success).toBe(false);
  });
});
