import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';

import { agentTemplatesQuery } from '@/features/secops/api';
import type { Agent } from '@/schemas/agent';
import type { AgentTemplate } from '@/schemas/secops';

export const CORE_TEAM = 'Core team';
/** An agent made by hand, outside the core roles. */
export const CUSTOM = 'Custom';

/** The roles of the five agents every workspace starts with. */
const CORE_ROLES = new Set(['planner', 'researcher', 'writer', 'engineer', 'reviewer']);

/** The catalogue's departments in the order they are shown; others follow by name, Custom last. */
export const DEPARTMENTS = [
  CORE_TEAM,
  'Finance & Risk',
  'IT, Cloud & Security',
  'Software, Data & AI',
  'Hardware & Manufacturing',
  'Supply Chain & Shipping',
  'Marketing & Sales',
  'Business, Legal & People',
] as const;

function rank(department: string): number {
  const i = (DEPARTMENTS as readonly string[]).indexOf(department);
  if (i >= 0) return i;
  return department === CUSTOM ? DEPARTMENTS.length + 1 : DEPARTMENTS.length;
}

export function compareDepartments(a: string, b: string): number {
  return rank(a) - rank(b) || a.localeCompare(b);
}

/**
 * The department of an agent: its template's, the core team for a core role made by hand, and
 * Custom otherwise. `null` while the templates are not known yet for an agent made from one.
 */
export function agentDepartment(
  agent: Agent,
  templates: ReadonlyMap<string, AgentTemplate> | undefined,
): string | null {
  const template = agent.template_id ? templates?.get(agent.template_id) : undefined;
  if (template) return template.department;
  if (agent.template_id && !templates) return null;
  if (!agent.template_id && CORE_ROLES.has(agent.role)) return CORE_TEAM;
  return CUSTOM;
}

/** Items in groups by department, in the departments' order; each group keeps the items' order. */
export function groupByDepartment<T>(
  items: readonly T[],
  departmentOf: (item: T) => string,
): { department: string; items: T[] }[] {
  const groups = new Map<string, T[]>();
  for (const item of items) {
    const department = departmentOf(item);
    const group = groups.get(department);
    if (group) group.push(item);
    else groups.set(department, [item]);
  }
  return [...groups.entries()]
    .sort(([a], [b]) => compareDepartments(a, b))
    .map(([department, list]) => ({ department, items: list }));
}

const byId = (templates: AgentTemplate[]) => new Map(templates.map((t) => [t.id, t]));

/** The role templates by id, from the cached catalogue. */
export function useTemplatesById(): ReadonlyMap<string, AgentTemplate> | undefined {
  const { data } = useQuery({ ...agentTemplatesQuery(), select: byId });
  return data;
}

/** Each agent's department, from the role templates (see {@link agentDepartment}). */
export function useAgentDepartments(): (agent: Agent) => string | null {
  const templates = useTemplatesById();
  return useCallback((agent: Agent) => agentDepartment(agent, templates), [templates]);
}
