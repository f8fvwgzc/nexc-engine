import { CITATION_KINDS, type CitationKind, type EvidenceVerdict } from '@/schemas/evidence';

/** One citation as written in an output: `[doc:0199…]`. */
export interface Citation {
  kind: CitationKind;
  id: string;
  /** `doc:0199…`, how the backend lists it. */
  key: string;
}

const ID = /^[A-Za-z0-9._:/-]{1,120}$/;

/** Reads one `kind:id` token; null when it is not one. */
export function parseCitation(token: string): Citation | null {
  const colon = token.indexOf(':');
  if (colon <= 0) return null;
  const kind = token.slice(0, colon);
  const id = token.slice(colon + 1);
  if (!(CITATION_KINDS as readonly string[]).includes(kind) || !ID.test(id)) return null;
  return { kind: kind as CitationKind, id, key: `${kind}:${id}` };
}

/** `text` with fenced code blocks and inline code spans blanked: an id in code is not a citation. */
function prose(text: string): string {
  return text.replace(/```[\s\S]*?(```|$)/g, ' ').replace(/`[^`\n]*`/g, ' ');
}

/**
 * Every citation in `text`, once each, in order of first appearance: the same reading the
 * backend's verifier does (CONTRACT §4). A bracket holding anything but ids — a source name, a
 * link, `[1]` — is not a citation.
 */
export function parseCitations(text: string): Citation[] {
  const found = new Map<string, Citation>();
  for (const match of prose(text).matchAll(/\[([^[\]\n]+)\]/g)) {
    const tokens = (match[1] ?? '').split(/[,;\s]+/).filter(Boolean);
    const parsed = tokens.map(parseCitation);
    if (tokens.length === 0 || parsed.some((c) => c === null)) continue;
    for (const citation of parsed as Citation[]) {
      if (!found.has(citation.key)) found.set(citation.key, citation);
    }
  }
  return [...found.values()];
}

/** Where a chip opens: the passage, the memory, the upstream node's panel, or the audit. */
export function citationHref(
  citation: Citation,
  where: { graphId?: string; runId?: string },
): string {
  switch (citation.kind) {
    case 'doc':
      return `/app/settings/knowledge?passage=${encodeURIComponent(citation.id)}`;
    case 'mem':
      return `/app/settings/memory?memory=${encodeURIComponent(citation.id)}`;
    case 'node':
      return where.graphId
        ? `/app/graphs/${where.graphId}?node=${encodeURIComponent(citation.id)}`
        : where.runId
          ? `/app/runs/${where.runId}`
          : '/app/runs';
    case 'fact':
      return `/app/settings/audit?fact=${encodeURIComponent(citation.id)}`;
  }
}

/** A chip's short text: the kind and the first characters of the id. */
export function citationLabel(citation: Citation): string {
  return `${citation.kind}:${citation.id.slice(0, 8)}`;
}

export const CITATION_KIND_META: Record<CitationKind, { label: string }> = {
  doc: { label: 'Document passage' },
  mem: { label: 'Memory' },
  node: { label: 'Upstream node' },
  fact: { label: 'Audit fact' },
};

export const VERDICT_META: Record<
  EvidenceVerdict,
  { label: string; description: string; tone: 'ok' | 'warn' | 'bad' }
> = {
  verified: {
    label: 'Verified',
    description: 'Enough of the claims cite what the node was given, and nothing was forged.',
    tone: 'ok',
  },
  partial: {
    label: 'Partial',
    description:
      'Some claims cite what the node was given; others cite nothing, or an id it never saw.',
    tone: 'warn',
  },
  unverified: {
    label: 'Unverified',
    description: 'No claim of the output rests on what the node was given.',
    tone: 'bad',
  },
};

/** The coverage as a whole percentage. */
export function coveragePct(coverage: number): number {
  return Math.round(coverage * 100);
}
