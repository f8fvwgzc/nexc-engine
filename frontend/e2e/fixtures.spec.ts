/**
 * The fixtures pass the app's own contract guard: every one parses with the zod schema the app
 * validates that response with, so a page never shows a contract error because of a fixture.
 */
import { expect, test } from '@playwright/test';
import { z } from 'zod';

import { a2aCounterpartSchema } from '../src/schemas/a2a';
import {
  a2aTaskSchema,
  agentApiTokenSchema,
  agentCardSchema,
  agentSchema,
} from '../src/schemas/agent';
import { alertDeliverySchema, alertDestinationSchema } from '../src/schemas/alert-routing';
import {
  artifactContentSchema,
  artifactFileSchema,
  artifactFolderSchema,
  artifactVersionSchema,
  runArtifactSchema,
} from '../src/schemas/artifact';
import { conversationDetailSchema, conversationSchema } from '../src/schemas/assistant';
import { auditEntrySchema, cannedQuestionSchema } from '../src/schemas/audit';
import { loginProvidersSchema, signInMethodsSchema, userSchema } from '../src/schemas/auth';
import { cycleSchema } from '../src/schemas/cycle';
import { graphSchema, graphSummarySchema } from '../src/schemas/graph';
import {
  daySummarySchema,
  infrastructureSchema,
  latencyReportSchema,
  timelineDaySchema,
  timelineEntrySchema,
} from '../src/schemas/insight';
import {
  issueEventSchema,
  issueSchema,
  issueStateSchema,
  labelSchema,
  notificationSchema,
  projectSchema,
} from '../src/schemas/issue';
import {
  documentSchema,
  knowledgeSettingsSchema,
  passageSchema,
  topicSchema,
} from '../src/schemas/knowledge';
import { mcpServerSchema } from '../src/schemas/mcp';
import { memorySchema, memoryTopicSchema } from '../src/schemas/memory';
import { ontologySessionSchema, ontologyStatusSchema } from '../src/schemas/ontology';
import { orchestratorStatusSchema } from '../src/schemas/orchestrator';
import {
  alertSchema,
  approvalSchema,
  policySchema,
  resolvedPolicySchema,
} from '../src/schemas/policy';
import { artifactSchema, runSchema } from '../src/schemas/run';
import {
  agentTemplateSchema,
  secopsOverviewSchema,
  secopsSpanPageSchema,
} from '../src/schemas/secops';
import { sessionPageSchema, sessionSchema, spanPayloadsSchema } from '../src/schemas/session';
import { llmSettingsSchema, modelCatalogSchema } from '../src/schemas/settings';
import { graphTemplateSchema } from '../src/schemas/template';
import { spanSchema, tracePageSchema, traceVerificationSchema } from '../src/schemas/trace';
import { usageReportSchema } from '../src/schemas/usage';
import {
  guardrailsSchema,
  teamMemberSchema,
  teamSchema,
  workspaceInviteSchema,
  workspaceMemberSchema,
  workspaceSchema,
} from '../src/schemas/workspace';
import { fixtures, type Fixtures } from './fixtures';

const many = <S extends z.ZodType>(schema: S) => z.array(schema);
const byTeam = <S extends z.ZodType>(schema: S) => z.record(z.string(), z.array(schema));

/**
 * The schema of each fixture. Those whose schema lives beside its query in `src/features` (the
 * platform console, account, search, transfers) are checked by the pages that parse them.
 */
const SCHEMAS: Partial<Record<keyof Fixtures, z.ZodType>> = {
  a2aCounterparts: many(a2aCounterpartSchema),
  a2aTasks: many(a2aTaskSchema),
  agentActivity: many(spanSchema),
  agentCard: agentCardSchema,
  agentTemplates: many(agentTemplateSchema),
  agentTokens: many(agentApiTokenSchema),
  agents: many(agentSchema),
  alertDestinations: many(alertDestinationSchema),
  alerts: many(alertSchema),
  approvals: many(approvalSchema),
  artifactContent: artifactContentSchema,
  artifactFiles: many(artifactFileSchema),
  artifactFolders: many(artifactFolderSchema),
  artifactVersions: many(artifactVersionSchema),
  artifacts: many(artifactSchema),
  audit: many(auditEntrySchema),
  auditQuestions: many(cannedQuestionSchema),
  auth: z.object({ user: userSchema, admin: userSchema }),
  conversation: conversationDetailSchema,
  conversations: many(conversationSchema),
  cycles: byTeam(cycleSchema),
  daySummary: daySummarySchema,
  deliveries: many(alertDeliverySchema),
  documents: many(documentSchema),
  graph: graphSchema,
  graphs: many(graphSummarySchema),
  guardrails: guardrailsSchema,
  inbox: many(notificationSchema),
  infrastructure: infrastructureSchema,
  invites: many(workspaceInviteSchema),
  issueEvents: many(issueEventSchema),
  issues: many(issueSchema),
  knowledgeSettings: knowledgeSettingsSchema,
  knowledgeTopics: many(topicSchema),
  labels: many(labelSchema),
  latency: latencyReportSchema,
  llmSettings: llmSettingsSchema,
  loginProviders: loginProvidersSchema,
  mcpServers: many(mcpServerSchema),
  members: many(workspaceMemberSchema),
  memories: many(memorySchema),
  memoryTopics: many(memoryTopicSchema),
  models: modelCatalogSchema,
  ontology: ontologyStatusSchema,
  ontologySession: ontologySessionSchema,
  orchestrator: orchestratorStatusSchema,
  passages: many(passageSchema),
  policies: many(policySchema),
  projects: many(projectSchema),
  resolvedPolicy: resolvedPolicySchema,
  run: runSchema,
  runArtifacts: many(runArtifactSchema),
  runs: many(runSchema),
  secopsOverview: secopsOverviewSchema,
  secopsOverviewEmpty: secopsOverviewSchema,
  secopsSpans: secopsSpanPageSchema,
  session: sessionSchema,
  sessions: sessionPageSchema,
  signInMethods: z.object({ user: signInMethodsSchema, admin: signInMethodsSchema }),
  spanPayloads: spanPayloadsSchema,
  states: byTeam(issueStateSchema),
  teamMembers: many(teamMemberSchema),
  teams: many(teamSchema),
  templates: many(graphTemplateSchema),
  timeline: many(timelineEntrySchema),
  timelineDays: many(timelineDaySchema),
  trace: tracePageSchema,
  traceVerify: traceVerificationSchema,
  usage: usageReportSchema,
  usageEmpty: usageReportSchema,
  workspaces: many(workspaceSchema),
};

for (const [name, schema] of Object.entries(SCHEMAS)) {
  test(`fixture ${name} matches the app's schema`, () => {
    const result = schema.safeParse(fixtures[name as keyof Fixtures]);
    expect(result.success ? [] : z.prettifyError(result.error).split('\n').slice(0, 12)).toEqual(
      [],
    );
  });
}
