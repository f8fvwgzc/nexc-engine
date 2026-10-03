import type { Agent } from '@/schemas/agent';

export interface AgentTreeNode {
  agent: Agent;
  reports: AgentTreeNode[];
}

/** Builds the reporting hierarchy; agents whose manager is missing (or cyclic) become roots. */
export function buildAgentTree(agents: Agent[]): AgentTreeNode[] {
  const byId = new Map(agents.map((a) => [a.id, { agent: a, reports: [] as AgentTreeNode[] }]));
  const roots: AgentTreeNode[] = [];
  for (const node of byId.values()) {
    const managerId = node.agent.reports_to;
    const manager = managerId ? byId.get(managerId) : undefined;
    if (manager && !isAncestor(node.agent.id, managerId, agents)) manager.reports.push(node);
    else roots.push(node);
  }
  return roots;
}

/** True if `ancestorId` appears in the management chain above `agentId`. */
function isAncestor(ancestorId: string, startId: string | null, agents: Agent[]): boolean {
  const byId = new Map(agents.map((a) => [a.id, a]));
  const seen = new Set<string>();
  let current = startId;
  while (current && !seen.has(current)) {
    if (current === ancestorId) return true;
    seen.add(current);
    current = byId.get(current)?.reports_to ?? null;
  }
  return false;
}

/** Agents that may become `agentId`'s manager (not itself, not one of its reports). */
export function managerCandidates(agents: Agent[], agentId: string | null): Agent[] {
  if (!agentId) return agents;
  return agents.filter((a) => a.id !== agentId && !isAncestor(agentId, a.id, agents));
}
