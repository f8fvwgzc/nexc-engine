import { BotIcon } from 'lucide-react';
import { useLayoutEffect, useRef } from 'react';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { useOverflowX } from '@/hooks/use-overflow-x';
import type { Agent } from '@/schemas/agent';

import { buildAgentTree, type AgentTreeNode } from '../agent-tree';
import { AgentActivity } from './agent-activity';
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
            <p className="truncate text-sm font-medium" title={agent.name}>
              {agent.name}
            </p>
            <p className="truncate text-xs text-muted-foreground" title={agent.title || agent.role}>
              {agent.title || agent.role}
            </p>
          </div>
        </div>
        <div className="flex items-center justify-between gap-2">
          <code className="truncate rounded bg-muted px-1.5 py-0.5 text-[11px]">{agent.role}</code>
          <AgentStatusBadge status={agent.status} />
        </div>
        <BudgetProgress spent={agent.spent_tokens} budget={agent.budget_tokens} />
      </button>
      <div className="border-t border-white/10 px-3 py-2">
        <AgentActivity agentId={agent.id} />
      </div>
    </GlassCard>
  );
}

function Branch({
  node,
  onEdit,
  isFirstRoot = false,
}: {
  node: AgentTreeNode;
  onEdit: (agent: Agent) => void;
  isFirstRoot?: boolean;
}) {
  return (
    <div className="flex flex-col items-center">
      <div data-org-root={isFirstRoot || undefined}>
        <AgentCard agent={node.agent} onEdit={onEdit} />
      </div>
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

/**
 * Reporting hierarchy (Paperclip-style org chart). A chart wider than the page scrolls sideways:
 * it opens centred on the first root, is a focusable region (the keyboard scrolls it) and says so.
 */
export function AgentOrgChart({
  agents,
  onEdit,
}: {
  agents: Agent[];
  onEdit: (agent: Agent) => void;
}) {
  const roots = buildAgentTree(agents);
  const scrollRef = useRef<HTMLDivElement>(null);
  const scrolls = useOverflowX(scrollRef);
  const firstRoot = roots[0]?.agent.id;
  useLayoutEffect(() => {
    const container = scrollRef.current;
    const root = container?.querySelector<HTMLElement>('[data-org-root]');
    if (!container || !root) return;
    container.scrollLeft = Math.max(
      0,
      root.offsetLeft + root.offsetWidth / 2 - container.clientWidth / 2,
    );
  }, [firstRoot]);
  return (
    <div className="space-y-2">
      <div
        ref={scrollRef}
        className="relative overflow-x-auto rounded-xl border bg-gradient-to-b from-muted/30 to-transparent p-6 focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none"
        {...(scrolls
          ? { tabIndex: 0, role: 'group', 'aria-label': 'Org chart, scrolls sideways' }
          : {})}
      >
        <ul className="mx-auto flex w-max gap-10" aria-label="Agent reporting structure">
          {roots.map((root, i) => (
            <li key={root.agent.id}>
              <Branch node={root} onEdit={onEdit} isFirstRoot={i === 0} />
            </li>
          ))}
        </ul>
      </div>
      {scrolls && (
        <p className="text-xs text-muted-foreground">
          The chart is wider than the page: scroll it sideways to see every agent.
        </p>
      )}
    </div>
  );
}
