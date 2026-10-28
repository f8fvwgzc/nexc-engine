import { BotIcon } from 'lucide-react';
import { useId, useLayoutEffect, useRef, useState } from 'react';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { Button } from '@/components/ui/button';
import { useOverflowX } from '@/hooks/use-overflow-x';
import type { Agent } from '@/schemas/agent';

import { buildAgentTree, type AgentTreeNode } from '../agent-tree';
import { groupByDepartment, useAgentDepartments } from '../departments';
import { AgentActivity } from './agent-activity';
import { AgentStatusBadge } from './agent-status-badge';
import { BudgetProgress } from './budget-progress';

/** A manager with more direct reports than this shows them by department, in columns. */
export const GROUP_REPORTS_OVER = 6;
/** Agents a department column shows before "Show more". */
const COLUMN_SHOWN = 6;

/**
 * Department columns (13rem each) under their manager: as many to a row as the page has room for
 * (one on a phone, up to four), wrapping onto more rows rather than widening the chart.
 */
const GROUPED_REPORTS =
  'flex max-w-[14.75rem] flex-wrap items-start justify-center gap-3 rounded-xl border border-dashed p-3 sm:max-w-[28.5rem] md:max-w-[42.25rem] xl:max-w-[56rem]';

/** The line from a manager down to each of its reports (one per `li`). */
const CONNECTOR =
  'relative px-3 pt-5 before:absolute before:top-0 before:left-1/2 before:h-5 before:w-px before:bg-border after:absolute after:inset-x-0 after:top-0 after:h-px after:bg-border first:after:left-1/2 last:after:right-1/2 only:after:hidden';

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

/** One agent in a department column: its name, title and status; its own reports under it. */
function CompactAgent({ node, onEdit }: { node: AgentTreeNode; onEdit: (agent: Agent) => void }) {
  const { agent } = node;
  return (
    <>
      <button
        type="button"
        onClick={() => onEdit(agent)}
        aria-label={`Edit ${agent.name}`}
        className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50"
      >
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[13px] font-medium" title={agent.name}>
            {agent.name}
          </span>
          <span
            className="block truncate text-[11px] text-muted-foreground"
            title={agent.title || agent.role}
          >
            {agent.title || agent.role}
          </span>
        </span>
        {agent.status !== 'active' && <AgentStatusBadge status={agent.status} />}
      </button>
      {node.reports.length > 0 && (
        <ul className="ml-3 border-l pl-1" aria-label={`Reports to ${agent.name}`}>
          {node.reports.map((child) => (
            <li key={child.agent.id}>
              <CompactAgent node={child} onEdit={onEdit} />
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

/** The direct reports of one department, as a column under their manager. */
function DepartmentColumn({
  department,
  nodes,
  onEdit,
}: {
  department: string;
  nodes: AgentTreeNode[];
  onEdit: (agent: Agent) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const headingId = useId();
  const shown = expanded ? nodes : nodes.slice(0, COLUMN_SHOWN);
  return (
    <section aria-labelledby={headingId} className="rounded-xl border bg-card/70 p-1.5 shadow-sm">
      <h3
        id={headingId}
        className="flex items-baseline justify-between gap-2 px-2 pt-0.5 pb-1 text-xs font-medium"
      >
        <span className="truncate" title={department}>
          {department}
        </span>
        <span className="shrink-0 text-muted-foreground tabular-nums">{nodes.length}</span>
      </h3>
      <ul>
        {shown.map((node) => (
          <li key={node.agent.id}>
            <CompactAgent node={node} onEdit={onEdit} />
          </li>
        ))}
      </ul>
      {nodes.length > COLUMN_SHOWN && (
        <Button
          variant="ghost"
          size="xs"
          className="mt-0.5 w-full text-muted-foreground"
          aria-expanded={expanded}
          onClick={() => setExpanded((v) => !v)}
        >
          {expanded ? 'Show fewer' : `Show ${nodes.length - COLUMN_SHOWN} more`}
        </Button>
      )}
    </section>
  );
}

function Branch({
  node,
  onEdit,
  departmentOf,
  isFirstRoot = false,
}: {
  node: AgentTreeNode;
  onEdit: (agent: Agent) => void;
  departmentOf: (agent: Agent) => string;
  isFirstRoot?: boolean;
}) {
  const grouped = node.reports.length > GROUP_REPORTS_OVER;
  return (
    <div className="flex flex-col items-center">
      <div data-org-root={isFirstRoot || undefined}>
        <AgentCard agent={node.agent} onEdit={onEdit} />
      </div>
      {grouped ? (
        <div className="relative pt-5 before:absolute before:top-0 before:left-1/2 before:h-5 before:w-px before:bg-border">
          <ul
            aria-label={`Reports to ${node.agent.name}, by department`}
            className={GROUPED_REPORTS}
          >
            {groupByDepartment(node.reports, (child) => departmentOf(child.agent)).map((group) => (
              <li key={group.department} className="w-52">
                <DepartmentColumn
                  department={group.department}
                  nodes={group.items}
                  onEdit={onEdit}
                />
              </li>
            ))}
          </ul>
        </div>
      ) : (
        node.reports.length > 0 && (
          <ul className="relative flex pt-5 before:absolute before:top-0 before:left-1/2 before:h-5 before:w-px before:bg-border">
            {node.reports.map((child) => (
              <li key={child.agent.id} className={CONNECTOR}>
                <Branch node={child} onEdit={onEdit} departmentOf={departmentOf} />
              </li>
            ))}
          </ul>
        )
      )}
    </div>
  );
}

/**
 * Reporting hierarchy (Paperclip-style org chart). A manager with many direct reports (the
 * Planner over the expert agents) shows them in a column per department, the columns wrapping
 * onto rows, so the chart stays about as wide as the page. A chart wider than the page scrolls
 * sideways: it opens centred on the first root, is a focusable region (the keyboard scrolls it)
 * and says so.
 */
export function AgentOrgChart({
  agents,
  onEdit,
}: {
  agents: Agent[];
  onEdit: (agent: Agent) => void;
}) {
  const roots = buildAgentTree(agents);
  const department = useAgentDepartments();
  const departmentOf = (agent: Agent) => department(agent) ?? 'Other';
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
              <Branch
                node={root}
                onEdit={onEdit}
                departmentOf={departmentOf}
                isFirstRoot={i === 0}
              />
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
