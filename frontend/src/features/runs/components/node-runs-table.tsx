import { Link } from 'react-router-dom';

import { StatusBadge } from '@/components/custom-ui/status-badge';
import { displayStatus } from '@/components/custom-ui/status-meta';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { durationBetween, formatDuration, formatTokens } from '@/lib/format';
import type { NodeRun } from '@/schemas/run';

import { EvidenceBadge } from './evidence';

interface NodeRunsTableProps {
  graphId: string;
  nodeRuns: NodeRun[];
  titles: Map<string, string>;
}

export function NodeRunsTable({ graphId, nodeRuns, titles }: NodeRunsTableProps) {
  return (
    <div className="overflow-x-auto rounded-xl border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Node</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="hidden text-right md:table-cell">Retries</TableHead>
            <TableHead className="hidden md:table-cell">Executor</TableHead>
            <TableHead className="hidden text-right sm:table-cell">Tokens in / out</TableHead>
            <TableHead className="hidden text-right lg:table-cell">Duration</TableHead>
            <TableHead>Evidence</TableHead>
            <TableHead className="hidden lg:table-cell">Error</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {nodeRuns.map((nr) => {
            const duration = durationBetween(nr.started_at, nr.finished_at);
            return (
              <TableRow key={nr.node_id}>
                <TableCell
                  className="max-w-56 truncate font-medium"
                  title={titles.get(nr.node_id) ?? undefined}
                >
                  <Link to={`/app/graphs/${graphId}`} className="hover:underline">
                    {titles.get(nr.node_id) ?? nr.node_id.slice(0, 8)}
                  </Link>
                </TableCell>
                <TableCell>
                  <StatusBadge status={displayStatus(nr.status, nr.cached)} />
                </TableCell>
                <TableCell className="hidden text-right tabular-nums md:table-cell">
                  {Math.max(0, nr.attempt - 1)}
                </TableCell>
                <TableCell className="hidden text-muted-foreground md:table-cell">
                  {nr.executor}
                </TableCell>
                <TableCell className="hidden text-right whitespace-nowrap tabular-nums sm:table-cell">
                  {formatTokens(nr.tokens_in)} / {formatTokens(nr.tokens_out)}
                </TableCell>
                <TableCell className="hidden text-right tabular-nums lg:table-cell">
                  {duration === null ? '—' : formatDuration(duration)}
                </TableCell>
                <TableCell>
                  {nr.evidence ? (
                    <EvidenceBadge evidence={nr.evidence} />
                  ) : (
                    <span className="text-muted-foreground">—</span>
                  )}
                </TableCell>
                <TableCell className="hidden max-w-64 lg:table-cell">
                  {nr.error ? (
                    <Tooltip>
                      <TooltipTrigger className="block max-w-full truncate text-left text-destructive">
                        {nr.error}
                      </TooltipTrigger>
                      <TooltipContent className="max-w-sm whitespace-pre-wrap">
                        {nr.error}
                      </TooltipContent>
                    </Tooltip>
                  ) : (
                    <span className="text-muted-foreground">—</span>
                  )}
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </div>
  );
}
