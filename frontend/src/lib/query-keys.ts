import type { MemoryQuery } from '@/schemas/memory';

/**
 * Query key factory. Keys are hierarchical so `invalidateQueries({ queryKey: qk.graphs.all })`
 * reaches every graph-scoped query.
 */
export const qk = {
  me: ['auth', 'me'] as const,
  settings: {
    llm: ['settings', 'llm'] as const,
    /** The settings in effect for the caller in one workspace. */
    effective: (workspaceId?: string) =>
      ['settings', 'llm', 'effective', workspaceId ?? 'none'] as const,
    workspace: (workspaceId: string) => ['settings', 'llm', 'workspace', workspaceId] as const,
    models: (workspaceId?: string) => ['settings', 'llm', 'models', workspaceId ?? 'none'] as const,
  },
  workspaces: {
    all: ['workspaces'] as const,
    members: (workspaceId: string) => ['workspaces', workspaceId, 'members'] as const,
    invites: (workspaceId: string) => ['workspaces', workspaceId, 'invites'] as const,
    teams: (workspaceId: string) => ['workspaces', workspaceId, 'teams'] as const,
    teamMembers: (workspaceId: string, teamId: string) =>
      ['workspaces', workspaceId, 'teams', teamId, 'members'] as const,
  },
  graphs: {
    all: ['graphs'] as const,
    /** Prefix of every graph list, whatever workspace it is scoped to. */
    lists: () => [...qk.graphs.all, 'list'] as const,
    list: (workspaceId?: string) => [...qk.graphs.lists(), workspaceId ?? 'all'] as const,
    detail: (graphId: string) => [...qk.graphs.all, 'detail', graphId] as const,
    suggestions: (graphId: string) => [...qk.graphs.all, 'detail', graphId, 'suggestions'] as const,
    analysis: (graphId: string) => [...qk.graphs.all, 'detail', graphId, 'analysis'] as const,
    plan: (graphId: string, planId: string) =>
      [...qk.graphs.all, 'detail', graphId, 'plans', planId] as const,
    runs: (graphId: string) => [...qk.graphs.all, 'detail', graphId, 'runs'] as const,
  },
  runs: {
    all: ['runs'] as const,
    detail: (runId: string) => [...qk.runs.all, runId] as const,
    artifacts: (runId: string) => [...qk.runs.all, runId, 'artifacts'] as const,
  },
  templates: ['templates'] as const,
  agents: { all: ['agents'] as const },
  memories: {
    all: ['memories'] as const,
    search: (query: MemoryQuery) => [...qk.memories.all, query] as const,
  },
  orchestrator: ['orchestrator', 'status'] as const,
};
