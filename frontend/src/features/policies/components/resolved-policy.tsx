import { useQuery } from '@tanstack/react-query';

import { Badge } from '@/components/ui/badge';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';
import { resolvedPolicyQuery } from '../api';
import { ruleRows } from '../rules';

const ORIGIN_META: Record<string, string> = {
  workspace: 'Workspace',
  team: 'Team',
  agent: 'Agent policy',
  capabilities: 'Agent capabilities',
};

/** The effective policy of one agent, rule by rule, with the level that set each. */
export function ResolvedPolicyView({
  workspaceId,
  agentId,
}: {
  workspaceId: string;
  agentId: string;
}) {
  const { data, isPending, error } = useQuery(resolvedPolicyQuery(workspaceId, agentId));
  if (isPending) return <Skeleton className="h-64 rounded-xl" />;
  if (error) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {errorMessage(error)}
      </p>
    );
  }
  const rows = ruleRows(data);
  return (
    <div className="space-y-3">
      <p className="text-xs text-muted-foreground">
        Levels in force:{' '}
        {data.levels.length === 0
          ? 'none (the agent’s own capabilities and the defaults)'
          : data.levels.map((l) => `${l.scope} v${l.version}`).join(' → ')}
        .
      </p>
      <div className="overflow-x-auto rounded-xl border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Rule</TableHead>
              <TableHead>Effective value</TableHead>
              <TableHead>Set by</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => {
              const origin = data.origins[row.path];
              return (
                <TableRow key={row.path}>
                  <TableCell className="font-mono text-xs">{row.path}</TableCell>
                  <TableCell className="max-w-md text-xs break-words">{row.value}</TableCell>
                  <TableCell>
                    <Badge variant={origin ? 'default' : 'outline'}>
                      {origin ? (ORIGIN_META[origin] ?? origin) : 'default'}
                    </Badge>
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
