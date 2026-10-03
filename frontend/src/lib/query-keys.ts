import type { MemoryQuery } from '@/schemas/memory';

/**
 * Query key factory. Keys are hierarchical so `invalidateQueries({ queryKey: qk.graphs.all })`
 * reaches every graph-scoped query.
 */
export const qk = {
  me: ['auth', 'me'] as const,
  settings: { llm: ['settings', 'llm'] as const },
  graphs: {
    all: ['graphs'] as const,
    list: () => [...qk.graphs.all, 'list'] as const,
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
