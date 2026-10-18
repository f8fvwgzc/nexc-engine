import { IdCardIcon, PencilIcon, Trash2Icon } from 'lucide-react';

import { IconButton } from '@/components/custom-ui/icon-button';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { formatRelative } from '@/lib/format';
import type { Agent } from '@/schemas/agent';

import { AgentStatusBadge } from './agent-status-badge';
import { BudgetProgress } from './budget-progress';

interface AgentsTableProps {
  agents: Agent[];
  onOpen: (agent: Agent) => void;
  onEdit: (agent: Agent) => void;
  onDelete: (agent: Agent) => void;
}

export function AgentsTable({ agents, onOpen, onEdit, onDelete }: AgentsTableProps) {
  const names = new Map(agents.map((a) => [a.id, a.name]));
  return (
    <div className="overflow-x-auto rounded-xl border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Agent</TableHead>
            <TableHead className="hidden sm:table-cell">Role</TableHead>
            <TableHead className="hidden 2xl:table-cell">Model</TableHead>
            <TableHead className="hidden lg:table-cell">Reports to</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="hidden md:table-cell">Budget</TableHead>
            <TableHead className="hidden lg:table-cell">Heartbeat</TableHead>
            <TableHead className="w-28">
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {agents.map((agent) => (
            <TableRow key={agent.id}>
              <TableCell className="min-w-40 whitespace-normal">
                <button
                  type="button"
                  className="text-left font-medium hover:underline"
                  onClick={() => onOpen(agent)}
                >
                  {agent.name}
                </button>
                <p className="text-xs text-muted-foreground">
                  {agent.title}
                  {agent.title ? ' · ' : ''}policy v{agent.policy_version}
                </p>
              </TableCell>
              <TableCell className="hidden sm:table-cell">
                <code className="rounded bg-muted px-1.5 py-0.5 text-xs">{agent.role}</code>
              </TableCell>
              <TableCell className="hidden text-muted-foreground 2xl:table-cell">
                {agent.model || 'default'} · {agent.runtime}
              </TableCell>
              <TableCell className="hidden text-muted-foreground lg:table-cell">
                {agent.reports_to ? (names.get(agent.reports_to) ?? '—') : '—'}
              </TableCell>
              <TableCell>
                <AgentStatusBadge status={agent.status} />
              </TableCell>
              <TableCell className="hidden md:table-cell">
                <BudgetProgress spent={agent.spent_tokens} budget={agent.budget_tokens} />
              </TableCell>
              <TableCell className="hidden whitespace-nowrap text-muted-foreground lg:table-cell">
                {agent.heartbeat_at ? formatRelative(agent.heartbeat_at) : 'never'}
              </TableCell>
              <TableCell>
                <div className="flex justify-end gap-1">
                  <IconButton
                    label={`Open the card of ${agent.name}`}
                    onClick={() => onOpen(agent)}
                  >
                    <IdCardIcon />
                  </IconButton>
                  <IconButton label={`Edit ${agent.name}`} onClick={() => onEdit(agent)}>
                    <PencilIcon />
                  </IconButton>
                  <IconButton
                    label={`Delete ${agent.name}`}
                    className="text-destructive"
                    onClick={() => onDelete(agent)}
                  >
                    <Trash2Icon />
                  </IconButton>
                </div>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
