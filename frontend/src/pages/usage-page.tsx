import { useSuspenseQuery } from '@tanstack/react-query';
import {
  ArrowDownToLineIcon,
  ArrowUpFromLineIcon,
  CoinsIcon,
  PhoneCallIcon,
  ScissorsIcon,
} from 'lucide-react';
import { Suspense, useState, type ReactNode } from 'react';

import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageHeader } from '@/components/custom-ui/page-header';
import { StatTile } from '@/components/custom-ui/stat-tile';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { usageQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { formatCost, formatInteger, formatTokens } from '@/lib/format';
import type { UsageReport, UsageTotals } from '@/schemas/usage';
import type { Workspace } from '@/schemas/workspace';

/** Rule of thumb for turning the measured characters into an approximate token count. */
const CHARS_PER_TOKEN = 4;

const PERIODS = [
  { value: '7', label: 'Last 7 days' },
  { value: '30', label: 'Last 30 days' },
  { value: '90', label: 'Last 90 days' },
  { value: '365', label: 'Last 12 months' },
];

const PURPOSE_LABEL: Record<UsageReport['by_purpose'][number]['key'], string> = {
  plan: 'Planning',
  node: 'Running nodes',
  memory: 'Extracting memories',
  assistant: 'Assistant',
};
const CREDENTIAL_LABEL: Record<UsageReport['by_credential'][number]['key'], string> = {
  user: 'Members’ own accounts',
  workspace: 'Workspace credential',
  server: 'Server default',
};

/** A breakdown as a table; the share column says how much of the period's tokens a row is. */
function Breakdown({
  title,
  caption,
  rows,
  total,
}: {
  title: string;
  caption?: string;
  rows: { id: string; label: ReactNode; totals: UsageTotals }[];
  total: UsageTotals;
}) {
  const all = total.tokens_in + total.tokens_out;
  return (
    <section className="space-y-2">
      <div>
        <h2 className="text-sm font-medium">{title}</h2>
        {caption && <p className="text-xs text-muted-foreground">{caption}</p>}
      </div>
      <div className="overflow-x-auto rounded-xl border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{title}</TableHead>
              <TableHead className="text-right">Calls</TableHead>
              <TableHead className="text-right">Tokens in</TableHead>
              <TableHead className="text-right">Tokens out</TableHead>
              <TableHead className="text-right">Share</TableHead>
              <TableHead className="text-right">Est. cost</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map(({ id, label, totals }) => (
              <TableRow key={id}>
                <TableCell className="font-medium">{label}</TableCell>
                <TableCell className="text-right tabular-nums">
                  {formatInteger(totals.calls)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {formatTokens(totals.tokens_in)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {formatTokens(totals.tokens_out)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {all > 0
                    ? `${Math.round(((totals.tokens_in + totals.tokens_out) / all) * 100)}%`
                    : '—'}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {formatCost(totals.cost_usd)}
                </TableCell>
              </TableRow>
            ))}
            {rows.length === 0 && (
              <TableRow>
                <TableCell colSpan={6} className="text-center text-muted-foreground">
                  Nothing in this period.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
    </section>
  );
}

function Report({ workspace, days }: { workspace: Workspace; days: number }) {
  const { data: report } = useSuspenseQuery(usageQuery(workspace.id, days));
  const { totals } = report;
  return (
    <div className="space-y-6">
      {report.scope === 'own' && (
        <p className="rounded-xl border p-3 text-sm text-muted-foreground">
          This is your own usage in {workspace.name}. Workspace admins see every member.
        </p>
      )}
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-5">
        <StatTile label="LLM calls" value={formatInteger(totals.calls)} icon={PhoneCallIcon} />
        <StatTile
          label="Tokens in"
          value={formatTokens(totals.tokens_in)}
          icon={ArrowDownToLineIcon}
        />
        <StatTile
          label="Tokens out"
          value={formatTokens(totals.tokens_out)}
          icon={ArrowUpFromLineIcon}
        />
        <StatTile
          label="Estimated cost"
          value={formatCost(totals.cost_usd)}
          icon={CoinsIcon}
          hint="List prices; 0 for local and CLI models"
        />
        <StatTile
          label="Context left out"
          value={`≈ ${formatTokens(Math.round(totals.context_chars_saved / CHARS_PER_TOKEN))}`}
          icon={ScissorsIcon}
          hint="Tokens of upstream text not sent"
        />
      </div>
      <Breakdown
        title="Paying account"
        caption="Whose credential the calls ran on."
        total={totals}
        rows={report.by_credential.map((s) => ({
          id: s.key,
          label: CREDENTIAL_LABEL[s.key],
          totals: s,
        }))}
      />
      {report.scope === 'workspace' && (
        <Breakdown
          title="Member"
          total={totals}
          rows={report.by_member.map((s) => ({
            id: s.key.user_id ?? s.key.name,
            label: s.key.name,
            totals: s,
          }))}
        />
      )}
      <Breakdown
        title="Model"
        total={totals}
        rows={report.by_model.map((s) => ({
          id: `${s.key.provider}:${s.key.model}`,
          label: (
            <>
              {s.key.model} <span className="text-muted-foreground">· {s.key.provider}</span>
            </>
          ),
          totals: s,
        }))}
      />
      <Breakdown
        title="Purpose"
        total={totals}
        rows={report.by_purpose.map((s) => ({
          id: s.key,
          label: PURPOSE_LABEL[s.key],
          totals: s,
        }))}
      />
      <Breakdown
        title="Day"
        caption="UTC days with usage, oldest first."
        total={totals}
        rows={report.by_day.map((s) => ({ id: s.key, label: s.key, totals: s }))}
      />
    </div>
  );
}

function Usage() {
  const { current } = useCurrentWorkspace();
  const [days, setDays] = useState('30');
  if (!current) return <PageSkeleton />;
  return (
    <div className="mx-auto w-full max-w-5xl space-y-6 p-4 sm:p-6">
      <PageHeader
        title="Usage"
        description={`Tokens spent in ${current.name}: by whom, on which model, for what, and on whose account.`}
        actions={
          <OptionSelect
            value={days}
            onValueChange={setDays}
            options={PERIODS}
            aria-label="Period"
            className="w-44"
          />
        }
      />
      <Suspense fallback={<PageSkeleton />}>
        <Report key={`${current.id}:${days}`} workspace={current} days={Number(days)} />
      </Suspense>
    </div>
  );
}

export default function UsagePage() {
  return (
    <>
      <Seo title="Usage" noIndex />
      <Usage />
    </>
  );
}
