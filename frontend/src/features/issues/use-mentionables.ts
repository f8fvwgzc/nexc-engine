import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';

import { agentsQuery } from '@/features/agents/api';
import {
  membersQuery,
  teamMembersQuery,
  teamsQuery,
  workspacesQuery,
} from '@/features/workspaces/api';
import type { Issue } from '@/schemas/issue';
import { isWorkspaceAdmin } from '@/schemas/workspace';
import { useAuthStore } from '@/stores/auth-store';

import type { Mentionable } from './mentions';

/** Someone the mention list offers, with a second line where one helps. */
export interface MentionCandidate extends Mentionable {
  /** An agent's role, and why it would not take work when it would not. */
  detail?: string;
}

/** Whom a comment on an issue can mention, and the live names its mentions show. */
export interface Mentionables {
  candidates: MentionCandidate[];
  /** The people the reader knows of, by id: live names for mentions that carry an old one. */
  people: Map<string, { name: string; email: string }>;
  /** The workspace's agents' names, by id. */
  agents: Map<string, string>;
  /**
   * Whether a mentioned agent is also asked for a plan: only members of the issue's team plan
   * its work (null until the teams are known).
   */
  canPlan: boolean | null;
}

const NOT_WORKING: Record<string, string> = {
  paused: 'paused',
  over_budget: 'over budget',
};

/**
 * Whom a comment on the issue can mention: the members who can see the issue, as the server
 * decides it (admins, everyone but guests on an open team, and the team's members; a guest, who
 * cannot list the workspace's members, gets the team's), and the workspace's agents that are not
 * quarantined. The team's members and the agents are read only once `load` is set (the composer
 * was used), so opening an issue costs no extra request.
 */
export function useMentionables(issue: Issue, load: boolean): Mentionables {
  const me = useAuthStore((s) => s.user?.id);
  const wid = issue.workspace_id;
  const { data: workspace } = useQuery({
    ...workspacesQuery(),
    select: (all) => all.find((w) => w.id === wid),
  });
  const role = workspace?.role;
  const { data: teams } = useQuery(teamsQuery(wid));
  const team = teams?.find((t) => t.id === issue.team_id);
  const { data: members } = useQuery({
    ...membersQuery(wid),
    enabled: role !== undefined && role !== 'guest',
  });
  const { data: teamMembers } = useQuery({
    ...teamMembersQuery(wid, issue.team_id),
    enabled: load,
  });
  const { data: agentList } = useQuery({ ...agentsQuery(wid), enabled: load });

  return useMemo(() => {
    const people = new Map<string, { name: string; email: string }>();
    for (const m of [...(members ?? []), ...(teamMembers ?? [])]) {
      if (!people.has(m.user_id)) people.set(m.user_id, { name: m.name, email: m.email });
    }
    const inTeam = new Set((teamMembers ?? []).map((t) => t.user_id));
    const open = team !== undefined && !team.private;
    const seeing = new Set(inTeam);
    for (const m of members ?? []) {
      if (m.suspended) continue;
      if (isWorkspaceAdmin(m.role) || (open && m.role !== 'guest')) seeing.add(m.user_id);
    }
    const candidates: MentionCandidate[] = [...seeing]
      .filter((id) => id !== me)
      .map((id) => ({ kind: 'user' as const, id, name: people.get(id)?.name ?? 'Someone' }))
      .sort((a, b) => a.name.localeCompare(b.name));
    const agents = new Map<string, string>();
    for (const a of agentList ?? []) {
      agents.set(a.id, a.name);
      if (a.status === 'quarantined') continue;
      const why = NOT_WORKING[a.status];
      candidates.push({
        kind: 'agent',
        id: a.id,
        name: a.name,
        detail: why ? `${a.role} · ${why}` : a.role,
      });
    }
    return { candidates, people, agents, canPlan: team ? team.role !== null : null };
  }, [members, teamMembers, agentList, team, me]);
}
