import { useQuery } from '@tanstack/react-query';
import { ActivityIcon, BotIcon, NetworkIcon, WorkflowIcon } from 'lucide-react';

import { StatTile } from '@/components/custom-ui/stat-tile';
import { Stagger } from '@/components/custom-ui/motion';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { BackendHealth } from '@/schemas/orchestrator';
import type { GraphSummary } from '@/schemas/graph';

import { orchestratorQuery } from '../api';

function HealthChip({ label, health }: { label: string; health: BackendHealth }) {
  const state = !health.enabled ? 'disabled' : health.ok ? 'ok' : 'down';
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs">
          <span
            className={cn(
              'size-1.5 rounded-full',
              state === 'ok' && 'bg-status-succeeded',
              state === 'down' && 'bg-status-failed',
              state === 'disabled' && 'bg-status-idle',
            )}
          />
          {label}
          <span className="sr-only">: {state}</span>
        </span>
      </TooltipTrigger>
      <TooltipContent>
        {health.detail ?? (state === 'disabled' ? 'Not enabled' : state)}
      </TooltipContent>
    </Tooltip>
  );
}

export function DashboardStats({ graphs }: { graphs: GraphSummary[] }) {
  const { data: status } = useQuery(orchestratorQuery());
  const nodes = graphs.reduce((sum, g) => sum + g.node_count, 0);
  const placeholder = '—';

  return (
    <div className="space-y-3">
      <Stagger className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <StatTile
          label="Graphs"
          value={graphs.length}
          icon={NetworkIcon}
          hint={`${nodes} nodes total`}
        />
        <StatTile
          label="Active runs"
          value={status?.active_runs ?? placeholder}
          icon={WorkflowIcon}
          hint={status ? `${status.running_nodes} nodes running` : undefined}
        />
        <StatTile
          label="Queue"
          value={status?.queue_depth ?? placeholder}
          icon={ActivityIcon}
          hint="nodes waiting"
        />
        <StatTile
          label="Agents"
          value={status?.agents_active ?? placeholder}
          icon={BotIcon}
          hint="active"
        />
      </Stagger>
      {status && (
        <div className="flex flex-wrap items-center gap-2" aria-label="Backend health">
          <HealthChip label="LLM" health={status.backends.llm} />
          <HealthChip label="Agent runtime" health={status.backends.agent_runtime} />
          <HealthChip label="Symphony" health={status.backends.symphony} />
        </div>
      )}
    </div>
  );
}
