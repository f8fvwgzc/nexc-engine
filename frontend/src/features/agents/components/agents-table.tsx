import { PencilIcon, Trash2Icon } from 'lucide-react';

import { Button } from '@/components/ui/button';
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
  onEdit: (agent: Agent) => void;
  onDelete: (agent: Agent) => void;
}

export function AgentsTable({ agents, onEdit, onDelete }: AgentsTableProps) {
  const names = new Map(agents.map((a) => [a.id, a.name]));
  return (
    <div className="overflow-x-auto rounded-xl border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Agent</TableHead>
            <TableHead>Role</TableHead>
            <TableHead>Model</TableHead>
            <TableHead>Reports to</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Budget</TableHead>
            <TableHead>Heartbeat</TableHead>
            <TableHead className="w-20">
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {agents.map((agent) => (
            <TableRow key={agent.id}>
              <TableCell>
                <p className="font-medium">{agent.name}</p>
                <p className="text-xs text-muted-foreground">{agent.title}</p>
              </TableCell>
              <TableCell>
                <code className="rounded bg-muted px-1.5 py-0.5 text-xs">{agent.role}</code>
              </TableCell>
              <TableCell className="text-muted-foreground">
                {agent.model || 'default'} · {agent.runtime}
              </TableCell>
              <TableCell className="text-muted-foreground">
                {agent.reports_to ? (names.get(agent.reports_to) ?? '—') : '—'}
              </TableCell>
              <TableCell>
                <AgentStatusBadge status={agent.status} />
              </TableCell>
              <TableCell>
                <BudgetProgress spent={agent.spent_tokens} budget={agent.budget_tokens} />
              </TableCell>
              <TableCell className="text-muted-foreground">
                {agent.heartbeat_at ? formatRelative(agent.heartbeat_at) : 'never'}
              </TableCell>
              <TableCell>
                <div className="flex justify-end gap-1">
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Edit ${agent.name}`}
                    onClick={() => onEdit(agent)}
                  >
                    <PencilIcon />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Delete ${agent.name}`}
                    className="text-destructive"
                    onClick={() => onDelete(agent)}
                  >
                    <Trash2Icon />
                  </Button>
                </div>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
