import {
  BotIcon,
  FingerprintIcon,
  GavelIcon,
  PackageXIcon,
  PlayIcon,
  SearchCheckIcon,
  ShieldAlertIcon,
  SirenIcon,
  UserCheckIcon,
} from 'lucide-react';
import { Link } from 'react-router-dom';

import { StatTile } from '@/components/custom-ui/stat-tile';
import { Skeleton } from '@/components/ui/skeleton';
import { formatInteger, formatRelative } from '@/lib/format';
import { SEVERITY_META } from '@/schemas/policy';
import type { SecopsOverview } from '@/schemas/secops';

/** The nine numbers at the top of the console. */
export function SecopsStats({ overview }: { overview: SecopsOverview | undefined }) {
  if (!overview) {
    return (
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-9" aria-busy>
        {Array.from({ length: 9 }, (_, i) => (
          <Skeleton key={i} className="h-[5.5rem] rounded-xl" />
        ))}
      </div>
    );
  }
  const {
    agents,
    runs_today: runs,
    alerts_open,
    denials_today,
    last_seal,
    evidence_today: evidence,
  } = overview;
  const denials = denials_today.reduce((sum, d) => sum + d.count, 0);
  const alerts = alerts_open.reduce((sum, a) => sum + a.count, 0);
  const unverified = evidence.unverified + evidence.partial;
  const sideAgents = [
    agents.quarantined > 0 ? `${agents.quarantined} quarantined` : null,
    agents.paused > 0 ? `${agents.paused} paused` : null,
    agents.over_budget > 0 ? `${agents.over_budget} over budget` : null,
  ].filter(Boolean);
  return (
    <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-9">
      <StatTile
        label="Agents"
        value={formatInteger(agents.active)}
        icon={BotIcon}
        hint={
          sideAgents.length > 0
            ? sideAgents.join(' · ')
            : `${agents.total} in all, every one active`
        }
      />
      <StatTile
        label="Runs today"
        value={formatInteger(runs.total)}
        icon={PlayIcon}
        hint={`${runs.succeeded} ok · ${runs.failed} failed${runs.active > 0 ? ` · ${runs.active} live` : ''} · ${overview.runs_week.total} this week`}
      />
      <StatTile
        label="Spans today"
        value={formatInteger(overview.spans_today)}
        icon={FingerprintIcon}
        hint="Every action, on the chain"
      />
      <StatTile
        label="Denials today"
        value={formatInteger(denials)}
        icon={ShieldAlertIcon}
        hint={denials_today[0] ? `most by ${denials_today[0].rule}` : 'nothing refused'}
        className={
          denials > 0
            ? 'border-[color-mix(in_oklch,var(--status-failed)_35%,transparent)]'
            : undefined
        }
      />
      <StatTile
        label="Approvals"
        value={formatInteger(overview.approvals_open)}
        icon={UserCheckIcon}
        hint={overview.approvals_open > 0 ? 'waiting for a decision' : 'nothing waits'}
      />
      <StatTile
        label="Alerts"
        value={formatInteger(alerts)}
        icon={SirenIcon}
        hint={
          alerts_open.length > 0
            ? alerts_open
                .map((a) => `${a.count} ${SEVERITY_META[a.severity].label.toLowerCase()}`)
                .join(' · ')
            : 'all acknowledged'
        }
        className={
          alerts > 0
            ? 'border-[color-mix(in_oklch,var(--status-failed)_35%,transparent)]'
            : undefined
        }
      />
      <StatTile
        label="Held artifacts"
        value={formatInteger(overview.artifacts_quarantined)}
        icon={PackageXIcon}
        hint="kept in by the DLP screen"
      />
      <StatTile
        label="Unverified today"
        value={formatInteger(unverified)}
        icon={SearchCheckIcon}
        hint={
          evidence.verified + unverified === 0
            ? 'no output answered yet'
            : `${evidence.verified} verified · ${evidence.partial} partial · ${evidence.unverified} unverified`
        }
        className={
          unverified > 0
            ? 'border-[color-mix(in_oklch,var(--status-queued)_45%,transparent)]'
            : undefined
        }
      />
      <StatTile
        label="Last seal"
        value={
          last_seal ? (
            <Link
              to={`/app/runs/${last_seal.run_id}`}
              className="font-mono text-base hover:underline"
              title={last_seal.trace_hash}
            >
              {last_seal.trace_hash.slice(0, 10)}
            </Link>
          ) : (
            '—'
          )
        }
        icon={GavelIcon}
        hint={
          last_seal?.sealed_at
            ? `sealed ${formatRelative(last_seal.sealed_at)}`
            : 'no run sealed yet'
        }
      />
    </div>
  );
}
