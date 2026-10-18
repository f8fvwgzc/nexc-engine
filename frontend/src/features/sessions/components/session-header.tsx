import {
  BrainCircuitIcon,
  ClockIcon,
  CoinsIcon,
  CpuIcon,
  ExternalLinkIcon,
  FileTextIcon,
  WrenchIcon,
} from 'lucide-react';
import { Link } from 'react-router-dom';

import { StatTile } from '@/components/custom-ui/stat-tile';
import { StatusBadge } from '@/components/custom-ui/status-badge';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { formatCost, formatDateTime, formatDuration, formatTokens } from '@/lib/format';
import type { Session } from '@/schemas/session';

/** The run's totals, its breakdown by model, and where else to open it. */
export function SessionHeader({ session }: { session: Session }) {
  const { rollup } = session;
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <StatusBadge status={session.status} />
        <Link
          to={`/app/graphs/${session.graph_id}`}
          className="font-medium underline-offset-4 hover:underline"
        >
          {session.graph_name}
        </Link>
        <span className="text-muted-foreground">
          · created {formatDateTime(session.created_at)}
        </span>
        {session.recording ? (
          <Badge variant="outline" className="gap-1">
            <FileTextIcon aria-hidden />
            Recorded
          </Badge>
        ) : (
          <Badge variant="ghost" className="gap-1 text-muted-foreground">
            <FileTextIcon aria-hidden />
            Not recorded
          </Badge>
        )}
        <Button variant="outline" size="sm" asChild className="ml-auto">
          <Link to={`/app/runs/${session.run_id}`}>
            <ExternalLinkIcon />
            Run page
          </Link>
        </Button>
      </div>
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <StatTile
          label="Duration"
          value={session.duration_ms === null ? '—' : formatDuration(session.duration_ms)}
          icon={ClockIcon}
          hint={
            session.started_at ? `started ${formatDateTime(session.started_at)}` : 'not started'
          }
        />
        <StatTile
          label="Tokens"
          value={formatTokens(rollup.tokens_in + rollup.tokens_out)}
          icon={CpuIcon}
          hint={`${formatTokens(rollup.tokens_in)} in · ${formatTokens(rollup.tokens_out)} out · ${formatTokens(rollup.tokens_cached)} cached`}
        />
        <StatTile
          label="Cost"
          value={formatCost(rollup.cost_usd)}
          icon={CoinsIcon}
          hint={`${session.by_model.length} model${session.by_model.length === 1 ? '' : 's'}`}
        />
        <StatTile
          label="Calls"
          value={`${rollup.llm_calls} · ${rollup.tool_calls}`}
          icon={WrenchIcon}
          hint={`model turns · tool calls${rollup.denials > 0 ? ` · ${rollup.denials} denied` : ''}`}
        />
      </div>
      {session.by_model.length > 0 && (
        <div className="overflow-x-auto rounded-xl border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>
                  <span className="flex items-center gap-1.5">
                    <BrainCircuitIcon className="size-3.5" aria-hidden />
                    Model
                  </span>
                </TableHead>
                <TableHead className="text-right">Turns</TableHead>
                <TableHead className="text-right">In</TableHead>
                <TableHead className="text-right">Out</TableHead>
                <TableHead className="text-right">Cached</TableHead>
                <TableHead className="text-right">Cost</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {session.by_model.map((m) => (
                <TableRow key={`${m.provider}/${m.model}`}>
                  <TableCell className="font-mono text-xs">
                    {m.model}
                    <span className="ml-1 text-muted-foreground">({m.provider})</span>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">{m.llm_calls}</TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatTokens(m.tokens_in)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatTokens(m.tokens_out)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatTokens(m.tokens_cached)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatCost(m.cost_usd)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  );
}
