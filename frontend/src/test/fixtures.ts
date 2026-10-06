import type { Agent } from '@/schemas/agent';
import type { AuthResponse, User } from '@/schemas/auth';
import type { Graph, GraphEdge, GraphNode, Ontology } from '@/schemas/graph';
import type { Memory } from '@/schemas/memory';
import type { OrchestratorStatus } from '@/schemas/orchestrator';
import type { Plan } from '@/schemas/plan';
import type { Artifact, NodeRun, Run } from '@/schemas/run';
import type { LlmSettings } from '@/schemas/settings';
import type { GraphTemplate } from '@/schemas/template';

/** Contract fixtures (CONTRACT §4) shared by schema and store tests. UUID v7 ids, RFC 3339 times. */
export const ids = {
  user: '0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b',
  workspace: '0190a1b2-c3d4-7e5f-8a9b-9c1d2e3f4a5b',
  graph: '0190a1b2-c3d4-7e5f-8a9b-1c1d2e3f4a5b',
  nodeA: '0190a1b2-c3d4-7e5f-8a9b-2c1d2e3f4a5b',
  nodeB: '0190a1b2-c3d4-7e5f-8a9b-3c1d2e3f4a5b',
  edge: '0190a1b2-c3d4-7e5f-8a9b-4c1d2e3f4a5b',
  plan: '0190a1b2-c3d4-7e5f-8a9b-5c1d2e3f4a5b',
  run: '0190a1b2-c3d4-7e5f-8a9b-6c1d2e3f4a5b',
  artifact: '0190a1b2-c3d4-7e5f-8a9b-7c1d2e3f4a5b',
  agent: '0190a1b2-c3d4-7e5f-8a9b-8c1d2e3f4a5b',
  memory: '0190a1b2-c3d4-7e5f-8a9b-9c1d2e3f4a5b',
} as const;

const at = '2026-10-03T12:00:00Z';

export const user: User = {
  id: ids.user,
  email: 'ada@example.com',
  name: 'Ada Lovelace',
  role: 'user',
  created_at: at,
};

export const authResponse: AuthResponse = {
  user,
  access_token: 'eyJhbGciOiJIUzI1NiJ9.e30.sig',
  expires_in: 900,
};

export const ontology: Ontology = {
  node_types: [
    {
      key: 'research',
      label: 'Research',
      description: 'Finding and summarising information.',
      color: '#0ea5e9',
      icon: 'book-open',
      default_role: 'researcher',
      default_executor: 'llm',
      stage: 1,
      produces_artifact: false,
      allow_code_exec: false,
    },
    {
      key: 'task',
      label: 'Task',
      description: 'A concrete step to carry out.',
      color: '#10b981',
      icon: 'list-todo',
      default_role: 'engineer',
      default_executor: 'llm',
      stage: 2,
      produces_artifact: false,
      allow_code_exec: false,
    },
  ],
  relation_types: [
    { key: 'depends_on', label: 'Depends on', description: '', blocking: true },
    { key: 'relates_to', label: 'Relates to', description: '', blocking: false },
  ],
};

export const node: GraphNode = {
  id: ids.nodeA,
  graph_id: ids.graph,
  title: 'Find sources',
  content: 'Look for [[Outline]] inputs',
  kind: 'research',
  tags: ['sources'],
  x: 120.5,
  y: -40,
  status: 'idle',
  agent_role: 'researcher',
  executor: 'agent',
  output: null,
  origin: 'user',
  created_at: at,
  updated_at: '2026-10-03T12:00:00.123456+00:00',
};

export const edge: GraphEdge = {
  id: ids.edge,
  graph_id: ids.graph,
  source: ids.nodeA,
  target: ids.nodeB,
  kind: 'depends_on',
  blocking: true,
  reason: 'the outline is built from the sources',
  origin: 'auto',
  weight: 1,
};

export const graph: Graph = {
  id: ids.graph,
  workspace_id: ids.workspace,
  team_id: null,
  name: 'Research report',
  description: 'docx',
  goal: 'Write a research docx',
  version: 3,
  ontology,
  nodes: [node, { ...node, id: ids.nodeB, title: 'Outline', kind: 'task' }],
  edges: [edge],
  created_at: at,
  updated_at: at,
};

export const nodeRun: NodeRun = {
  node_id: ids.nodeA,
  status: 'succeeded',
  attempt: 1,
  executor: 'agent',
  tokens_in: 1200,
  tokens_out: 340,
  cached: false,
  error: null,
  started_at: at,
  finished_at: '2026-10-03T12:00:09Z',
  output_preview: 'Found 12 sources',
};

export const run: Run = {
  id: ids.run,
  graph_id: ids.graph,
  status: 'running',
  tokens_in: 1200,
  tokens_out: 340,
  cost_usd: 0.0123,
  started_at: at,
  finished_at: null,
  created_at: at,
  node_runs: [
    nodeRun,
    {
      ...nodeRun,
      node_id: ids.nodeB,
      status: 'queued',
      tokens_in: 0,
      tokens_out: 0,
      started_at: null,
      finished_at: null,
      output_preview: null,
    },
  ],
};

export const plan: Plan = {
  id: ids.plan,
  graph_id: ids.graph,
  status: 'ready',
  summary: 'Split research',
  nodes: [
    {
      ref: 'n1',
      existing_id: null,
      title: 'Citations',
      content: '',
      kind: 'task',
      agent_role: 'writer',
      executor: 'llm',
      tags: [],
    },
  ],
  ontology: { node_types: [], relation_types: [] },
  edges: [
    {
      source_ref: 'n1',
      target_ref: 'n2',
      kind: 'depends_on',
      reason: 'citations need the sources',
    },
  ],
  error: null,
  created_at: at,
};

export const artifact: Artifact = {
  id: ids.artifact,
  run_id: ids.run,
  node_id: ids.nodeA,
  path: 'report.docx',
  size: 20480,
  mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  created_at: at,
};

export const agent: Agent = {
  id: ids.agent,
  name: 'Ada',
  role: 'researcher',
  title: 'Head of Research',
  model: 'claude-opus-5',
  system_prompt: 'You research.',
  reports_to: null,
  budget_tokens: 200000,
  spent_tokens: 1540,
  status: 'active',
  runtime: 'python',
  heartbeat_at: null,
  created_at: at,
};

export const memory: Memory = {
  id: ids.memory,
  scope: 'graph',
  graph_id: ids.graph,
  node_id: null,
  kind: 'fact',
  content: 'The report targets executives.',
  importance: 0.8,
  access_count: 3,
  score: 0.91,
  created_at: at,
  updated_at: at,
};

const health = { enabled: true, ok: true, url: 'http://localhost:8090', detail: null };

export const orchestratorStatus: OrchestratorStatus = {
  demo_mode: true,
  queue_depth: 2,
  running_nodes: 1,
  active_runs: 1,
  agents_active: 3,
  backends: {
    llm: health,
    agent_runtime: health,
    symphony: { ...health, enabled: false, url: null },
  },
};

export const template: GraphTemplate = {
  id: 'research-report-docx',
  name: 'Research report (.docx)',
  description: 'Sources → outline → draft → docx',
  category: 'writing',
  node_count: 7,
  tags: ['docx', 'research'],
};

export const llmSettings: LlmSettings = {
  provider: 'demo',
  model: 'demo',
  base_url: null,
  has_api_key: false,
  key_hint: null,
  source: 'none',
  scope: 'server',
};
