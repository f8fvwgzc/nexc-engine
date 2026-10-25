import { IdCardIcon, PencilIcon, SearchIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';

import { IconButton } from '@/components/custom-ui/icon-button';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { formatRelative } from '@/lib/format';
import { matchesWords } from '@/lib/utils';
import type { Agent } from '@/schemas/agent';

import { compareDepartments, useAgentDepartments } from '../departments';
import { AgentStatusBadge } from './agent-status-badge';
import { BudgetProgress } from './budget-progress';

const ALL = '__all__';

interface AgentsTableProps {
  agents: Agent[];
  onOpen: (agent: Agent) => void;
  onEdit: (agent: Agent) => void;
  onDelete: (agent: Agent) => void;
}

/** Every agent, with its department and skills; a search and a department narrow the rows. */
export function AgentsTable({ agents, onOpen, onEdit, onDelete }: AgentsTableProps) {
  const names = new Map(agents.map((a) => [a.id, a.name]));
  const departmentOf = useAgentDepartments();
  const [search, setSearch] = useState('');
  const [department, setDepartment] = useState<string>(ALL);
  const departments = [
    ...new Set(agents.map(departmentOf).filter((d): d is string => d !== null)),
  ].sort(compareDepartments);
  const rows = agents.filter((agent) => {
    const own = departmentOf(agent);
    return (
      (department === ALL || own === department) &&
      matchesWords([agent.name, agent.role, agent.title, own ?? ''], search)
    );
  });
  return (
    <div className="space-y-3">
      <div className="flex flex-col gap-2 sm:flex-row">
        <InputGroup className="sm:max-w-xs">
          <InputGroupAddon>
            <SearchIcon aria-hidden />
          </InputGroupAddon>
          <InputGroupInput
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search agents"
            aria-label="Search agents"
          />
        </InputGroup>
        {departments.length > 1 && (
          <OptionSelect
            className="sm:w-56"
            aria-label="Filter by department"
            value={department}
            onValueChange={setDepartment}
            options={[
              { value: ALL, label: 'All departments' },
              ...departments.map((d) => ({ value: d, label: d })),
            ]}
          />
        )}
        <p className="self-center text-xs text-muted-foreground tabular-nums" role="status">
          {rows.length !== agents.length && `${rows.length} of ${agents.length} agents`}
        </p>
      </div>
      <div className="overflow-x-auto rounded-xl border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Agent</TableHead>
              <TableHead className="hidden sm:table-cell">Role</TableHead>
              <TableHead className="hidden md:table-cell">Department</TableHead>
              <TableHead className="hidden xl:table-cell">Skills</TableHead>
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
            {rows.length === 0 && (
              <TableRow>
                <TableCell colSpan={10} className="py-6 text-center text-muted-foreground">
                  No agent matches the search.
                </TableCell>
              </TableRow>
            )}
            {rows.map((agent) => (
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
                <TableCell className="hidden whitespace-normal text-muted-foreground md:table-cell">
                  {departmentOf(agent) ?? '—'}
                </TableCell>
                <TableCell className="hidden text-muted-foreground tabular-nums xl:table-cell">
                  {agent.skills.length === 0
                    ? 'none'
                    : `${agent.skills.length} skill${agent.skills.length === 1 ? '' : 's'}`}
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
    </div>
  );
}
