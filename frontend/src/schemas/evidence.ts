import { z } from 'zod';

/** How much of an output rests on what the node was given (CONTRACT §4, AGENTSECOPS §2). */
export const EVIDENCE_VERDICTS = ['verified', 'partial', 'unverified'] as const;
export const evidenceVerdictSchema = z.enum(EVIDENCE_VERDICTS);
export type EvidenceVerdict = z.infer<typeof evidenceVerdictSchema>;

/** What the verifier found about a node's output. */
export const evidenceSchema = z.object({
  verdict: evidenceVerdictSchema,
  /** 0–1: claims citing what the node was given, over the claims not declared the model's own. */
  coverage: z.number().min(0).max(1),
  claims: z.number().int().nonnegative(),
  cited_claims: z.number().int().nonnegative(),
  declared_claims: z.number().int().nonnegative(),
  /** Distinct ids cited that the node was given: `doc:…`, `mem:…`, `node:…`, `fact:…`. */
  cited: z.array(z.string()),
  /** Distinct ids cited that the node was never given. */
  unknown: z.array(z.string()),
  given: z.number().int().nonnegative(),
  min_coverage: z.number().min(0).max(1),
});
export type Evidence = z.infer<typeof evidenceSchema>;

/** What a citation points at: a document passage, a memory, an upstream node or an audit fact. */
export const CITATION_KINDS = ['doc', 'mem', 'node', 'fact'] as const;
export type CitationKind = (typeof CITATION_KINDS)[number];
