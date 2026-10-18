import type { Agent, AgentStatus } from '@/schemas/agent';

/** Agents matching the text (name, role, title, owner, model) and the status, in the list's order. */
export function filterAgents(
  agents: Agent[],
  text: string,
  status: AgentStatus | 'all',
  ownerNames: Map<string, string>,
): Agent[] {
  const needle = text.trim().toLowerCase();
  return agents.filter((agent) => {
    if (status !== 'all' && agent.status !== status) return false;
    if (!needle) return true;
    const owner = agent.owner_user_id ? (ownerNames.get(agent.owner_user_id) ?? '') : '';
    return [agent.name, agent.role, agent.title, owner, agent.model]
      .join(' ')
      .toLowerCase()
      .includes(needle);
  });
}
