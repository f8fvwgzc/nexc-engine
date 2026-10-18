import { useQuery } from '@tanstack/react-query';
import { BotIcon, PlusIcon, ShieldBanIcon, ShieldCheckIcon } from 'lucide-react';
import { useId, useMemo, useState } from 'react';
import { toast } from 'sonner';

import { CopyButton } from '@/components/custom-ui/copy-button';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card';
import { Input } from '@/components/ui/input';
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select';
import { AgentStatusBadge } from '@/features/agents/components/agent-status-badge';
import { useQuarantineAgent } from '@/features/agents/hooks/use-agent-mutations';
import { membersQuery, teamsQuery } from '@/features/workspaces/api';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative, formatTokens } from '@/lib/format';
import { cn } from '@/lib/utils';
import { AGENT_STATUSES, type Agent, type AgentStatus } from '@/schemas/agent';

import { filterAgents } from '../registry';
import { AgentTemplateDialog } from './agent-template-dialog';
import { Panel, PanelEmpty } from './console-panel';

const STATUS_LABEL: Record<AgentStatus, string> = {
  active: 'Active',
  paused: 'Paused',
  over_budget: 'Over budget',
  quarantined: 'Quarantined',
};

/** The risk score as a number that explains itself on hover or focus. */
function RiskScore({ agent }: { agent: Agent }) {
  if (agent.risk_updated_at === null) {
    return <span className="text-xs text-muted-foreground">not scored</span>;
  }
  const high = agent.risk_score >= 6;
  return (
    <HoverCard openDelay={150}>
      <HoverCardTrigger asChild>
        <button
          type="button"
          className={cn(
            'rounded px-1 text-sm font-medium tabular-nums underline decoration-dotted underline-offset-2 outline-none focus-visible:ring-3 focus-visible:ring-ring/50',
            high && 'text-destructive',
          )}
          aria-label={`Risk ${agent.risk_score.toFixed(1)} of 10`}
        >
          {agent.risk_score.toFixed(1)}
        </button>
      </HoverCardTrigger>
      <HoverCardContent align="start" className="w-72 text-xs">
        <p className="font-medium">
          Risk {agent.risk_score.toFixed(1)} / 10 · {formatRelative(agent.risk_updated_at)}
        </p>
        {agent.risk_reasons.length === 0 ? (
          <p className="text-muted-foreground">Nothing unusual in its latest run.</p>
        ) : (
          <ul className="mt-1 list-disc space-y-0.5 pl-4">
            {agent.risk_reasons.map((reason) => (
              <li key={reason}>{reason}</li>
            ))}
          </ul>
        )}
      </HoverCardContent>
    </HoverCard>
  );
}

interface RegistryPanelProps {
  workspaceId: string;
  agents: Agent[];
  /** Whether the viewer may quarantine and release (workspace admins). */
  admin: boolean;
  /** Whether the viewer may add agents (members and above). */
  member: boolean;
  onOpen: (agent: Agent) => void;
  className?: string;
}

/**
 * Every agent of the workspace as a row: who it is, who answers for it, what it runs on, its
 * status, risk, policy version, heartbeat and budget, with quarantine and release at hand.
 */
export function RegistryPanel({
  workspaceId,
  agents,
  admin,
  member,
  onOpen,
  className,
}: RegistryPanelProps) {
  const searchId = useId();
  const [text, setText] = useState('');
  const [status, setStatus] = useState<AgentStatus | 'all'>('all');
  const [adding, setAdding] = useState(false);
  const { data: members = [] } = useQuery(membersQuery(workspaceId));
  const { data: teams = [] } = useQuery(teamsQuery(workspaceId));
  const owners = useMemo(() => new Map(members.map((m) => [m.user_id, m.name])), [members]);
  const teamKeys = useMemo(() => new Map(teams.map((t) => [t.id, t.key])), [teams]);
  const shown = filterAgents(agents, text, status, owners);
  const quarantine = useQuarantineAgent();
  const toggle = (agent: Agent) => {
    const release = agent.status === 'quarantined';
    quarantine.mutate(
      { id: agent.id, release },
      {
        onSuccess: (next) =>
          toast.success(
            next.status === 'quarantined'
              ? `${next.name} is quarantined: running nodes were cancelled`
              : `${next.name} is back in service`,
          ),
        onError: (err) => toast.error(errorMessage(err)),
      },
    );
  };
  return (
    <Panel
      id="secops-registry"
      title="Registry"
      icon={BotIcon}
      count={agents.length}
      className={className}
      bodyClassName="p-0"
      actions={
        <>
          <label htmlFor={searchId} className="sr-only">
            Filter agents
          </label>
          <Input
            id={searchId}
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Name, role, owner…"
            className="h-7 w-36 text-xs sm:w-44"
          />
          <NativeSelect
            aria-label="Status"
            size="sm"
            value={status}
            onChange={(e) => setStatus(e.target.value as AgentStatus | 'all')}
          >
            <NativeSelectOption value="all">Any status</NativeSelectOption>
            {AGENT_STATUSES.map((s) => (
              <NativeSelectOption key={s} value={s}>
                {STATUS_LABEL[s]}
              </NativeSelectOption>
            ))}
          </NativeSelect>
          {member && (
            <Button size="sm" variant="outline" onClick={() => setAdding(true)}>
              <PlusIcon />
              From template
            </Button>
          )}
        </>
      }
    >
      {shown.length === 0 ? (
        <PanelEmpty
          icon={BotIcon}
          action={
            agents.length === 0 && member ? (
              <Button size="sm" onClick={() => setAdding(true)}>
                <PlusIcon />
                Create from a template
              </Button>
            ) : undefined
          }
        >
          {agents.length === 0
            ? 'No agents in this workspace yet: each starts from a role template with its policy.'
            : 'No agent matches the filter.'}
        </PanelEmpty>
      ) : (
        <ul className="divide-y" aria-label="Agents">
          {shown.map((agent) => {
            const quarantined = agent.status === 'quarantined';
            const budgetLeft =
              agent.budget_tokens > 0
                ? Math.max(0, agent.budget_tokens - agent.spent_tokens)
                : null;
            return (
              <li
                key={agent.id}
                className="grid grid-cols-[minmax(0,1fr)_auto] gap-x-3 gap-y-1 px-3 py-2 text-xs sm:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)_minmax(0,1fr)_auto]"
              >
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <button
                      type="button"
                      onClick={() => onOpen(agent)}
                      title={agent.name}
                      className="truncate text-sm font-medium hover:underline focus-visible:underline focus-visible:outline-none"
                    >
                      {agent.name}
                    </button>
                    <AgentStatusBadge status={agent.status} />
                    <code className="rounded bg-muted px-1 py-0.5 text-[11px]">{agent.role}</code>
                  </div>
                  <p className="flex min-w-0 items-center gap-1 font-mono text-[11px] text-muted-foreground">
                    <span className="truncate">{agent.principal_id}</span>
                    <CopyButton value={agent.principal_id} label="Copy principal" />
                  </p>
                </div>
                <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 text-muted-foreground">
                  <dt>Owner</dt>
                  <dd className="truncate text-foreground">
                    {agent.owner_user_id ? (owners.get(agent.owner_user_id) ?? '—') : 'nobody'}
                  </dd>
                  <dt>Team</dt>
                  <dd className="truncate text-foreground">
                    {agent.team_id ? (teamKeys.get(agent.team_id) ?? '—') : '—'}
                  </dd>
                  <dt>Model</dt>
                  <dd className="truncate text-foreground">
                    {agent.model || 'default'} · {agent.runtime}
                  </dd>
                </dl>
                <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 text-muted-foreground">
                  <dt>Risk</dt>
                  <dd>
                    <RiskScore agent={agent} />
                  </dd>
                  <dt>Policy</dt>
                  <dd>
                    <Badge variant="outline" className="h-4 px-1 text-[10px]">
                      v{agent.policy_version}
                    </Badge>
                  </dd>
                  <dt>Heartbeat</dt>
                  <dd className="text-foreground">
                    {agent.heartbeat_at ? formatRelative(agent.heartbeat_at) : 'never'}
                  </dd>
                  <dt>Budget</dt>
                  <dd className="text-foreground tabular-nums">
                    {budgetLeft === null ? 'no limit' : `${formatTokens(budgetLeft)} left`}
                  </dd>
                </dl>
                {admin && (
                  <div className="col-span-full flex justify-end gap-1 sm:col-span-1 sm:self-center">
                    <Button
                      size="xs"
                      variant={quarantined ? 'default' : 'outline'}
                      className={cn(!quarantined && 'text-destructive')}
                      disabled={quarantine.isPending}
                      onClick={() => toggle(agent)}
                      aria-label={`${quarantined ? 'Release' : 'Quarantine'} ${agent.name}`}
                    >
                      {quarantined ? <ShieldCheckIcon /> : <ShieldBanIcon />}
                      {quarantined ? 'Release' : 'Quarantine'}
                    </Button>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {adding && (
        <AgentTemplateDialog
          workspaceId={workspaceId}
          open={adding}
          onOpenChange={setAdding}
          onCreated={(agent) => onOpen(agent)}
        />
      )}
    </Panel>
  );
}
