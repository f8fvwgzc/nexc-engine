/**
 * The API fixtures the UI suite serves, typed against the OpenAPI schema (`src/lib/api/schema.d.ts`).
 *
 * A JSON import types every string as `string`, so each fixture is checked against its schema with
 * the string literals widened (`Json<T>`): a missing or misnamed required field, or a wrong type,
 * fails `pnpm typecheck`. The enum values themselves are checked at run time by the app's own zod
 * schemas, which `fixtures.spec.ts` applies to every fixture.
 */
import type { components } from '../../src/lib/api/schema';

import a2aCounterparts from './a2a-counterparts.json' with { type: 'json' };
import a2aTasks from './a2a-tasks.json' with { type: 'json' };
import accountActivity from './account-activity.json' with { type: 'json' };
import accountSessions from './account-sessions.json' with { type: 'json' };
import agentActivity from './agent-activity.json' with { type: 'json' };
import agentCard from './agent-card.json' with { type: 'json' };
import agentTemplates from './agent-templates.json' with { type: 'json' };
import agentTokens from './agent-tokens.json' with { type: 'json' };
import agents from './agents.json' with { type: 'json' };
import alertDestinations from './alert-destinations.json' with { type: 'json' };
import alerts from './alerts.json' with { type: 'json' };
import approvals from './approvals.json' with { type: 'json' };
import artifacts from './artifacts.json' with { type: 'json' };
import auditQuestions from './audit-questions.json' with { type: 'json' };
import audit from './audit.json' with { type: 'json' };
import auth from './auth.json' with { type: 'json' };
import conversation from './conversation.json' with { type: 'json' };
import conversations from './conversations.json' with { type: 'json' };
import cycles from './cycles.json' with { type: 'json' };
import daySummary from './day-summary.json' with { type: 'json' };
import deliveries from './deliveries.json' with { type: 'json' };
import documents from './documents.json' with { type: 'json' };
import graph from './graph.json' with { type: 'json' };
import graphs from './graphs.json' with { type: 'json' };
import guardrails from './guardrails.json' with { type: 'json' };
import inbox from './inbox.json' with { type: 'json' };
import infrastructure from './infrastructure.json' with { type: 'json' };
import invites from './invites.json' with { type: 'json' };
import issueEvents from './issue-events.json' with { type: 'json' };
import issues from './issues.json' with { type: 'json' };
import knowledgeSettings from './knowledge-settings.json' with { type: 'json' };
import knowledgeTopics from './knowledge-topics.json' with { type: 'json' };
import labels from './labels.json' with { type: 'json' };
import latency from './latency.json' with { type: 'json' };
import llmSettings from './llm-settings.json' with { type: 'json' };
import loginProviders from './login-providers.json' with { type: 'json' };
import mapItems from './map-items.json' with { type: 'json' };
import mapNeighbourhood from './map-neighbourhood.json' with { type: 'json' };
import map from './map.json' with { type: 'json' };
import mcpServers from './mcp-servers.json' with { type: 'json' };
import members from './members.json' with { type: 'json' };
import memories from './memories.json' with { type: 'json' };
import memoryTopics from './memory-topics.json' with { type: 'json' };
import models from './models.json' with { type: 'json' };
import orchestrator from './orchestrator.json' with { type: 'json' };
import passages from './passages.json' with { type: 'json' };
import platformEvents from './platform-events.json' with { type: 'json' };
import platformUsers from './platform-users.json' with { type: 'json' };
import platformWorkspace from './platform-workspace.json' with { type: 'json' };
import platformWorkspaces from './platform-workspaces.json' with { type: 'json' };
import policies from './policies.json' with { type: 'json' };
import projects from './projects.json' with { type: 'json' };
import resolvedPolicy from './resolved-policy.json' with { type: 'json' };
import run from './run.json' with { type: 'json' };
import runs from './runs.json' with { type: 'json' };
import search from './search.json' with { type: 'json' };
import secopsOverviewEmpty from './secops-overview.empty.json' with { type: 'json' };
import secopsOverview from './secops-overview.json' with { type: 'json' };
import secopsSpans from './secops-spans.json' with { type: 'json' };
import session from './session.json' with { type: 'json' };
import sessions from './sessions.json' with { type: 'json' };
import signInMethods from './sign-in-methods.json' with { type: 'json' };
import spanPayloads from './span-payloads.json' with { type: 'json' };
import states from './states.json' with { type: 'json' };
import teamMembers from './team-members.json' with { type: 'json' };
import teams from './teams.json' with { type: 'json' };
import templates from './templates.json' with { type: 'json' };
import timelineDays from './timeline-days.json' with { type: 'json' };
import timeline from './timeline.json' with { type: 'json' };
import traceVerify from './trace-verify.json' with { type: 'json' };
import trace from './trace.json' with { type: 'json' };
import transfers from './transfers.json' with { type: 'json' };
import twoFactor from './two-factor.json' with { type: 'json' };
import usageEmpty from './usage.empty.json' with { type: 'json' };
import usage from './usage.json' with { type: 'json' };
import workspaces from './workspaces.json' with { type: 'json' };

type Schemas = components['schemas'];

/** `T` as a JSON import types it: string literal unions become `string`. */
export type Json<T> = T extends string
  ? string
  : T extends number
    ? number
    : T extends boolean
      ? boolean
      : T extends null
        ? null
        : T extends readonly (infer U)[]
          ? Json<U>[]
          : // A free-form object (`type: object`, no properties) is typed `{ [key: string]: never }`.
            T extends { [key: string]: never }
            ? Record<string, unknown>
            : T extends object
              ? { [K in keyof T]: Json<T[K]> }
              : T;

type One<K extends keyof Schemas> = Json<Schemas[K]>;
type Many<K extends keyof Schemas> = Json<Schemas[K]>[];
type ByTeam<K extends keyof Schemas> = Record<string, Json<Schemas[K]>[]>;

/** Checks a fixture against its type and gives it that type. */
const typed = <T>(value: T): T => value;

/** Every fixture, by the name the mock API serves it under. */
export const fixtures = {
  a2aCounterparts: typed<Many<'A2aCounterpart'>>(a2aCounterparts),
  a2aTasks: typed<Many<'A2aTask'>>(a2aTasks),
  accountActivity: typed<Many<'AccountEvent'>>(accountActivity),
  accountSessions: typed<One<'Sessions'>>(accountSessions),
  agentActivity: typed<Many<'Span'>>(agentActivity),
  agentCard: typed<One<'AgentCard'>>(agentCard),
  agentTemplates: typed<Many<'AgentTemplate'>>(agentTemplates),
  agentTokens: typed<Many<'AgentApiToken'>>(agentTokens),
  agents: typed<Many<'Agent'>>(agents),
  alertDestinations: typed<Many<'AlertDestination'>>(alertDestinations),
  alerts: typed<Many<'Alert'>>(alerts),
  approvals: typed<Many<'Approval'>>(approvals),
  artifacts: typed<Many<'Artifact'>>(artifacts),
  audit: typed<Many<'AuditEntry'>>(audit),
  auditQuestions: typed<Many<'CannedQuestion'>>(auditQuestions),
  auth: typed<{ user: One<'User'>; admin: One<'User'> }>(auth),
  conversation: typed<One<'ConversationDetail'>>(conversation),
  conversations: typed<Many<'Conversation'>>(conversations),
  cycles: typed<ByTeam<'Cycle'>>(cycles),
  daySummary: typed<One<'DaySummary'>>(daySummary),
  deliveries: typed<Many<'AlertDelivery'>>(deliveries),
  documents: typed<Many<'Document'>>(documents),
  graph: typed<One<'Graph'>>(graph),
  graphs: typed<Many<'GraphSummary'>>(graphs),
  guardrails: typed<One<'Guardrails'>>(guardrails),
  inbox: typed<Many<'Notification'>>(inbox),
  infrastructure: typed<One<'Infrastructure'>>(infrastructure),
  invites: typed<Many<'WorkspaceInvite'>>(invites),
  issueEvents: typed<Many<'IssueEvent'>>(issueEvents),
  issues: typed<Many<'Issue'>>(issues),
  knowledgeSettings: typed<One<'KnowledgeSettings'>>(knowledgeSettings),
  knowledgeTopics: typed<Many<'Topic'>>(knowledgeTopics),
  labels: typed<Many<'Label'>>(labels),
  latency: typed<One<'LatencyReport'>>(latency),
  llmSettings: typed<One<'LlmSettings'>>(llmSettings),
  loginProviders: typed<One<'LoginProviders'>>(loginProviders),
  map: typed<One<'WorkspaceMap'>>(map),
  mapItems: typed<Many<'MapItem'>>(mapItems),
  mapNeighbourhood: typed<One<'MapNeighbourhood'>>(mapNeighbourhood),
  mcpServers: typed<Many<'McpServer'>>(mcpServers),
  members: typed<Many<'WorkspaceMember'>>(members),
  memories: typed<Many<'Memory'>>(memories),
  memoryTopics: typed<Many<'MemoryTopic'>>(memoryTopics),
  models: typed<One<'ModelCatalog'>>(models),
  orchestrator: typed<One<'OrchestratorStatus'>>(orchestrator),
  passages: typed<Many<'Passage'>>(passages),
  platformEvents: typed<Many<'PlatformEvent'>>(platformEvents),
  platformUsers: typed<Many<'PlatformUser'>>(platformUsers),
  platformWorkspace: typed<One<'PlatformWorkspaceDetail'>>(platformWorkspace),
  platformWorkspaces: typed<Many<'PlatformWorkspace'>>(platformWorkspaces),
  policies: typed<Many<'Policy'>>(policies),
  projects: typed<Many<'Project'>>(projects),
  resolvedPolicy: typed<One<'Resolved'>>(resolvedPolicy),
  run: typed<One<'Run'>>(run),
  runs: typed<Many<'Run'>>(runs),
  search: typed<Many<'SearchHit'>>(search),
  secopsOverview: typed<One<'SecopsOverview'>>(secopsOverview),
  secopsOverviewEmpty: typed<One<'SecopsOverview'>>(secopsOverviewEmpty),
  secopsSpans: typed<One<'SecopsSpanPage'>>(secopsSpans),
  session: typed<One<'Session'>>(session),
  sessions: typed<One<'SessionPage'>>(sessions),
  signInMethods: typed<{ user: One<'SignInMethods'>; admin: One<'SignInMethods'> }>(signInMethods),
  spanPayloads: typed<One<'SpanPayloads'>>(spanPayloads),
  states: typed<ByTeam<'IssueState'>>(states),
  teamMembers: typed<Many<'TeamMember'>>(teamMembers),
  teams: typed<Many<'Team'>>(teams),
  templates: typed<Many<'GraphTemplate'>>(templates),
  timeline: typed<Many<'TimelineEntry'>>(timeline),
  timelineDays: typed<Many<'TimelineDay'>>(timelineDays),
  trace: typed<One<'TracePage'>>(trace),
  traceVerify: typed<One<'TraceVerification'>>(traceVerify),
  transfers: typed<Many<'Transfer'>>(transfers),
  twoFactor: typed<One<'TwoFactor'>>(twoFactor),
  usage: typed<One<'UsageReport'>>(usage),
  usageEmpty: typed<One<'UsageReport'>>(usageEmpty),
  workspaces: typed<Many<'Workspace'>>(workspaces),
};

export type Fixtures = typeof fixtures;
