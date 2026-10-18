import { useQuery } from '@tanstack/react-query';
import { GavelIcon } from 'lucide-react';
import { useState } from 'react';
import { Link } from 'react-router-dom';

import { ErrorState } from '@/components/custom-ui/error-state';
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select';
import { ScrollRegion } from '@/components/custom-ui/scroll-region';
import { Skeleton } from '@/components/ui/skeleton';
import { policiesQuery } from '@/features/policies/api';
import { DocumentDiff } from '@/features/policies/components/policy-versions';
import { ResolvedPolicyView } from '@/features/policies/components/resolved-policy';
import { teamsQuery } from '@/features/workspaces/api';
import type { Agent } from '@/schemas/agent';
import type { PolicyScope } from '@/schemas/policy';
import type { SecopsOverview } from '@/schemas/secops';

import { Panel, PanelEmpty } from './console-panel';

type Level = { scope: 'workspace' } | { scope: 'team' | 'agent'; id: string };

/** Any two versions of one level, side by side as a diff. */
function VersionDiff({ workspaceId, level }: { workspaceId: string; level: Level }) {
  const {
    data: versions,
    isPending,
    error,
    refetch,
  } = useQuery(
    policiesQuery(workspaceId, level.scope, level.scope === 'workspace' ? undefined : level.id),
  );
  const [from, setFrom] = useState<number | null>(null);
  const [to, setTo] = useState<number | null>(null);
  if (isPending) return <Skeleton className="h-24 rounded-lg" />;
  if (error) {
    return (
      <ErrorState
        compact
        title="Couldn’t load the versions"
        error={error}
        onRetry={() => void refetch()}
      />
    );
  }
  if (versions.length === 0) {
    return <p className="text-xs text-muted-foreground">No version set at this level.</p>;
  }
  const latest = versions[0]?.version ?? 1;
  const after = versions.find((v) => v.version === (to ?? latest)) ?? versions[0] ?? null;
  const before =
    versions.find((v) => v.version === (from ?? (after ? after.version - 1 : 0))) ?? null;
  if (!after) return null;
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <label className="flex items-center gap-1">
          From
          <NativeSelect
            size="sm"
            aria-label="Earlier version"
            value={before?.version ?? 0}
            onChange={(e) => setFrom(Number(e.target.value))}
          >
            <NativeSelectOption value={0}>nothing</NativeSelectOption>
            {versions.map((v) => (
              <NativeSelectOption key={v.id} value={v.version}>
                v{v.version}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </label>
        <label className="flex items-center gap-1">
          to
          <NativeSelect
            size="sm"
            aria-label="Later version"
            value={after.version}
            onChange={(e) => setTo(Number(e.target.value))}
          >
            {versions.map((v) => (
              <NativeSelectOption key={v.id} value={v.version}>
                v{v.version}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </label>
        <span className="text-muted-foreground">
          v{after.version} by {after.author_name || 'someone'}
        </span>
      </div>
      <DocumentDiff before={before} after={after} className="max-h-64" />
    </div>
  );
}

interface PoliciesPanelProps {
  workspaceId: string;
  agents: Agent[];
  overview: SecopsOverview | undefined;
  admin: boolean;
  className?: string;
}

/**
 * What governs each agent: the resolved policy rule by rule with the level that set it, and
 * the diff between any two versions of a level. Editing stays on the Policies settings page.
 */
export function PoliciesPanel({
  workspaceId,
  agents,
  overview,
  admin,
  className,
}: PoliciesPanelProps) {
  const { data: teams = [] } = useQuery({ ...teamsQuery(workspaceId), enabled: admin });
  const [agentId, setAgentId] = useState<string | null>(null);
  const [level, setLevel] = useState<Level>({ scope: 'workspace' });
  const picked = agents.find((a) => a.id === agentId) ?? agents[0] ?? null;
  const levelValue = level.scope === 'workspace' ? 'workspace' : `${level.scope}:${level.id}`;
  const pickLevel = (next: string) => {
    if (next === 'workspace') setLevel({ scope: 'workspace' });
    else {
      const [scope, id] = next.split(':') as [PolicyScope, string];
      if (scope === 'team' || scope === 'agent') setLevel({ scope, id });
    }
  };
  const inForce = overview?.policy_versions ?? [];
  return (
    <Panel
      id="secops-policies"
      title="Policies"
      icon={GavelIcon}
      className={className}
      hint={
        inForce.length === 0
          ? 'no version set: capabilities and defaults'
          : `${inForce.length} level${inForce.length === 1 ? '' : 's'} in force`
      }
      actions={
        admin ? (
          <Link
            to="/app/settings/policies"
            className="text-xs text-muted-foreground hover:underline"
          >
            Edit in Settings
          </Link>
        ) : undefined
      }
    >
      {agents.length === 0 ? (
        <PanelEmpty>No agents to resolve a policy for.</PanelEmpty>
      ) : (
        <div className="space-y-4">
          <section className="space-y-2" aria-label="Resolved policy">
            <div className="flex flex-wrap items-center gap-2">
              <p className="text-xs text-muted-foreground">Effective policy of</p>
              <NativeSelect
                size="sm"
                aria-label="Agent"
                value={picked?.id ?? ''}
                onChange={(e) => setAgentId(e.target.value)}
              >
                {agents.map((a) => (
                  <NativeSelectOption key={a.id} value={a.id}>
                    {a.name}
                  </NativeSelectOption>
                ))}
              </NativeSelect>
            </div>
            {picked && (
              <ScrollRegion
                label={`Effective policy of ${picked.name}`}
                className="max-h-80 overflow-y-auto"
              >
                <ResolvedPolicyView workspaceId={workspaceId} agentId={picked.id} />
              </ScrollRegion>
            )}
          </section>
          {admin && (
            <section className="space-y-2 border-t pt-3" aria-label="Version diff">
              <div className="flex flex-wrap items-center gap-2">
                <p className="text-xs text-muted-foreground">Versions of</p>
                <NativeSelect
                  size="sm"
                  aria-label="Level"
                  value={levelValue}
                  onChange={(e) => pickLevel(e.target.value)}
                >
                  <NativeSelectOption value="workspace">the workspace</NativeSelectOption>
                  {teams.map((team) => (
                    <NativeSelectOption key={team.id} value={`team:${team.id}`}>
                      team {team.name}
                    </NativeSelectOption>
                  ))}
                  {agents.map((agent) => (
                    <NativeSelectOption key={agent.id} value={`agent:${agent.id}`}>
                      agent {agent.name}
                    </NativeSelectOption>
                  ))}
                </NativeSelect>
              </div>
              <VersionDiff key={levelValue} workspaceId={workspaceId} level={level} />
            </section>
          )}
        </div>
      )}
    </Panel>
  );
}
