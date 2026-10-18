import { useQuery } from '@tanstack/react-query';
import { GavelIcon } from 'lucide-react';
import { useState } from 'react';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { agentsQuery } from '@/features/agents/api';
import { PolicyVersions } from '@/features/policies/components/policy-versions';
import { ResolvedPolicyView } from '@/features/policies/components/resolved-policy';
import { teamsQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import type { PolicyScope } from '@/schemas/policy';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

/** The level being edited: `workspace`, or a team or agent by id. */
type Level = { scope: 'workspace' } | { scope: 'team' | 'agent'; id: string };

function Policies({ workspace }: { workspace: Workspace }) {
  const { data: teams = [] } = useQuery(teamsQuery(workspace.id));
  const { data: agents = [] } = useQuery(agentsQuery(workspace.id));
  const [level, setLevel] = useState<Level>({ scope: 'workspace' });
  const [resolveFor, setResolveFor] = useState<string | null>(null);
  const value = level.scope === 'workspace' ? 'workspace' : `${level.scope}:${level.id}`;
  const pick = (next: string) => {
    if (next === 'workspace') setLevel({ scope: 'workspace' });
    else {
      const [scope, id] = next.split(':') as [PolicyScope, string];
      if (scope === 'team' || scope === 'agent') setLevel({ scope, id });
    }
  };
  const levelName =
    level.scope === 'workspace'
      ? 'the workspace'
      : level.scope === 'team'
        ? `team ${teams.find((t) => t.id === level.id)?.name ?? ''}`
        : `agent ${agents.find((a) => a.id === level.id)?.name ?? ''}`;
  const agentForResolve = resolveFor ?? agents[0]?.id ?? null;
  return (
    <div className="space-y-8">
      <section className="space-y-3" aria-labelledby="versions">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 id="versions" className="text-sm font-medium">
            Versions
          </h2>
          <Select value={value} onValueChange={pick}>
            <SelectTrigger className="w-64" aria-label="Level">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="workspace">Whole workspace</SelectItem>
              {teams.map((team) => (
                <SelectItem key={team.id} value={`team:${team.id}`}>
                  Team {team.name}
                </SelectItem>
              ))}
              {agents.map((agent) => (
                <SelectItem key={agent.id} value={`agent:${agent.id}`}>
                  Agent {agent.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <PolicyVersions
          key={value}
          workspaceId={workspace.id}
          scope={level.scope}
          scopeId={level.scope === 'workspace' ? null : level.id}
          levelName={levelName}
        />
      </section>
      <section className="space-y-3" aria-labelledby="effective">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 id="effective" className="text-sm font-medium">
            Effective policy
          </h2>
          {agents.length > 0 && (
            <Select value={agentForResolve ?? undefined} onValueChange={setResolveFor}>
              <SelectTrigger className="w-64" aria-label="Agent">
                <SelectValue placeholder="Pick an agent" />
              </SelectTrigger>
              <SelectContent>
                {agents.map((agent) => (
                  <SelectItem key={agent.id} value={agent.id}>
                    {agent.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        </div>
        {agentForResolve ? (
          <ResolvedPolicyView workspaceId={workspace.id} agentId={agentForResolve} />
        ) : (
          <p className="text-sm text-muted-foreground">No agents to resolve a policy for yet.</p>
        )}
      </section>
    </div>
  );
}

export default function PoliciesSettingsPage() {
  const { current } = useCurrentWorkspace();
  if (!current) return <PageSkeleton />;
  const allowed = isWorkspaceAdmin(current.role);
  return (
    <div className="mx-auto w-full max-w-5xl space-y-6 p-4 sm:p-6">
      <Seo title="Policies" noIndex />
      <PageHeader
        title="Policies"
        description={`What agents in ${current.name} may do, as versioned rules: the workspace sets the outer limits, teams and agents only narrow them. Every version is audited.`}
      />
      {allowed ? (
        <Policies key={current.id} workspace={current} />
      ) : (
        <EmptyState
          icon={GavelIcon}
          title="Only admins set policies"
          description="Ask an owner or admin of this workspace."
        />
      )}
    </div>
  );
}
