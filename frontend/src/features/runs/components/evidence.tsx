import {
  BrainIcon,
  FileTextIcon,
  GitBranchIcon,
  SearchCheckIcon,
  type LucideIcon,
} from 'lucide-react';
import { Link } from 'react-router-dom';

import { Badge } from '@/components/ui/badge';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { CitationKind, Evidence } from '@/schemas/evidence';

import {
  CITATION_KIND_META,
  VERDICT_META,
  citationHref,
  citationLabel,
  coveragePct,
  parseCitation,
  type Citation,
} from '../evidence';

const KIND_ICON: Record<CitationKind, LucideIcon> = {
  doc: FileTextIcon,
  mem: BrainIcon,
  node: GitBranchIcon,
  fact: SearchCheckIcon,
};

const TONE_CLASS = {
  ok: 'border-[color-mix(in_oklch,var(--status-succeeded)_45%,transparent)] [--badge-color:var(--status-succeeded)] text-badge',
  warn: 'border-[color-mix(in_oklch,var(--status-queued)_55%,transparent)] [--badge-color:var(--status-queued)] text-badge',
  bad: 'border-destructive/40 [--badge-color:var(--destructive)] text-badge',
} as const;

/** The verdict as a badge, with the coverage; hover for what it means. */
export function EvidenceBadge({ evidence, className }: { evidence: Evidence; className?: string }) {
  const meta = VERDICT_META[evidence.verdict];
  const counted = evidence.claims - evidence.declared_claims;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge
          variant="outline"
          data-verdict={evidence.verdict}
          className={cn('gap-1 tabular-nums', TONE_CLASS[meta.tone], className)}
        >
          {meta.label}
          <span className="font-normal">{coveragePct(evidence.coverage)}%</span>
        </Badge>
      </TooltipTrigger>
      <TooltipContent className="max-w-xs">
        <p>{meta.description}</p>
        <p className="mt-1 text-xs opacity-80">
          {evidence.cited_claims} of {counted} claims cite what the node was given
          {evidence.declared_claims > 0 && `, ${evidence.declared_claims} declared the model's own`}
          ; {evidence.given} given; {Math.round(evidence.min_coverage * 100)}% required
        </p>
      </TooltipContent>
    </Tooltip>
  );
}

interface ChipProps {
  citation: Citation;
  /** The graph the cited upstream node belongs to (its chip opens the node's panel). */
  graphId?: string;
  runId?: string;
  /** Node titles, for upstream chips. */
  titles?: Map<string, string>;
  /** The id was cited but never given to the node. */
  unknown?: boolean;
}

/** One citation as a chip that opens what it points at. */
export function CitationChip({ citation, graphId, runId, titles, unknown = false }: ChipProps) {
  const Icon = KIND_ICON[citation.kind];
  const title = citation.kind === 'node' ? titles?.get(citation.id) : undefined;
  const text = title ?? citationLabel(citation);
  const what = `${CITATION_KIND_META[citation.kind].label} ${citation.id}${unknown ? ' — not among what the node was given' : ''}`;
  const className = cn(
    'inline-flex h-6 max-w-56 items-center gap-1 rounded-full border px-2 font-mono text-[11px] outline-none hover:bg-muted/60 focus-visible:border-ring',
    unknown && 'border-destructive/40 text-destructive line-through decoration-destructive/60',
  );
  if (unknown) {
    return (
      <span className={className} title={what} data-citation={citation.key} data-unknown>
        <Icon className="size-3 shrink-0" aria-hidden />
        <span className="truncate">{text}</span>
      </span>
    );
  }
  return (
    <Link
      to={citationHref(citation, { graphId, runId })}
      className={className}
      title={what}
      data-citation={citation.key}
    >
      <Icon className="size-3 shrink-0" aria-hidden />
      <span className="truncate">{text}</span>
    </Link>
  );
}

interface ChipsProps {
  /** Ids as the backend lists them (`doc:…`), or citations already read out of a text. */
  cited: (string | Citation)[];
  unknown?: (string | Citation)[];
  graphId?: string;
  runId?: string;
  titles?: Map<string, string>;
  className?: string;
}

function toCitation(item: string | Citation): Citation | null {
  return typeof item === 'string' ? parseCitation(item) : item;
}

/** The ids an output cites, as chips; the ones it was never given are crossed out. */
export function CitationChips({
  cited,
  unknown = [],
  graphId,
  runId,
  titles,
  className,
}: ChipsProps) {
  const known = cited.map(toCitation).filter((c): c is Citation => c !== null);
  const forged = unknown.map(toCitation).filter((c): c is Citation => c !== null);
  if (known.length === 0 && forged.length === 0) return null;
  return (
    <ul className={cn('flex flex-wrap gap-1', className)} aria-label="Citations">
      {known.map((citation) => (
        <li key={citation.key}>
          <CitationChip citation={citation} graphId={graphId} runId={runId} titles={titles} />
        </li>
      ))}
      {forged.map((citation) => (
        <li key={`unknown-${citation.key}`}>
          <CitationChip
            citation={citation}
            graphId={graphId}
            runId={runId}
            titles={titles}
            unknown
          />
        </li>
      ))}
    </ul>
  );
}

/** The verdict with its chips and, when ids were forged, what they were. */
export function EvidencePanel({
  evidence,
  graphId,
  runId,
  titles,
  className,
}: {
  evidence: Evidence;
  graphId?: string;
  runId?: string;
  titles?: Map<string, string>;
  className?: string;
}) {
  return (
    <div className={cn('space-y-2', className)} data-testid="evidence-panel">
      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <EvidenceBadge evidence={evidence} />
        <span>
          {evidence.cited_claims} of {evidence.claims - evidence.declared_claims} claims cited
          {evidence.declared_claims > 0 && ` · ${evidence.declared_claims} declared`}
          {evidence.given > 0 ? ` · ${evidence.given} given` : ' · nothing was given'}
        </span>
      </div>
      <CitationChips
        cited={evidence.cited}
        unknown={evidence.unknown}
        graphId={graphId}
        runId={runId}
        titles={titles}
      />
      {evidence.unknown.length > 0 && (
        <p className="text-xs text-destructive" role="note">
          {evidence.unknown.length === 1
            ? 'One id was cited that the node was never given:'
            : `${evidence.unknown.length} ids were cited that the node was never given:`}{' '}
          <span className="font-mono">{evidence.unknown.join(', ')}</span>
        </p>
      )}
    </div>
  );
}
