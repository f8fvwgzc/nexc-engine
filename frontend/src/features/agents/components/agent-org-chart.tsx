import { BotIcon } from 'lucide-react';

import { GlassCard } from '@/components/custom-ui/glass-card';
import type { Agent } from '@/schemas/agent';

import { buildAgentTree, type AgentTreeNode } from '../agent-tree';
import { AgentStatusBadge } from './agent-status-badge';
import { BudgetProgress } from './budget-progress';

function AgentCard({ agent, onEdit }: { agent: Agent; onEdit: (agent: Agent) => void }) {
  return (
    <GlassCard interactive className="w-60">
      <button
        type="button"
        onClick={() => onEdit(agent)}
        className="flex w-full flex-col gap-2 rounded-[inherit] p-3 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
        aria-label={`Edit ${agent.name}`}
      >
        <div className="flex items-center gap-2">
          <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-brand/10 text-brand">
            <BotIcon className="size-4" aria-hidden />
          </span>
          <div className="min-w-0">
            <p className="truncate text-sm font-medium">{agent.name}</p>
            <p className="truncate text-xs text-muted-foreground">{agent.title || agent.role}</p>
          </div>
        </div>
        <div className="flex items-center justify-between gap-2">
          <code className="truncate rounded bg-muted px-1.5 py-0.5 text-[11px]">{agent.role}</code>
          <AgentStatusBadge status={agent.status} />
        </div>
        <BudgetProgress spent={agent.spent_tokens} budget={agent.budget_tokens} />
      </button>
    </GlassCard>
  );
}

function Branch({ node, onEdit }: { node: AgentTreeNode; onEdit: (agent: Agent) => void }) {
  return (
    <div className="flex flex-col items-center">
      <AgentCard agent={node.agent} onEdit={onEdit} />
      {node.reports.length > 0 && (
        <ul className="relative flex pt-5 before:absolute before:top-0 before:left-1/2 before:h-5 before:w-px before:bg-border">
          {node.reports.map((child) => (
            <li
              key={child.agent.id}
              className="relative px-3 pt-5 before:absolute before:top-0 before:left-1/2 before:h-5 before:w-px before:bg-border after:absolute after:inset-x-0 after:top-0 after:h-px after:bg-border first:after:left-1/2 last:after:right-1/2 only:after:hidden"
            >
              <Branch node={child} onEdit={onEdit} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Reporting hierarchy (Paperclip-style org chart); horizontally scrollable on small screens. */
export function AgentOrgChart({
  agents,
  onEdit,
}: {
  agents: Agent[];
  onEdit: (agent: Agent) => void;
}) {
  const roots = buildAgentTree(agents);
  return (
    <div className="overflow-x-auto rounded-xl border bg-gradient-to-b from-muted/30 to-transparent p-6">
      <ul className="mx-auto flex w-max gap-10" aria-label="Agent reporting structure">
        {roots.map((root) => (
          <li key={root.agent.id}>
            <Branch node={root} onEdit={onEdit} />
          </li>
        ))}
      </ul>
    </div>
  );
}
