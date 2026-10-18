import type { SecopsSpan } from '@/schemas/secops';

/** `2026-10-18T09:30` of a local date-time input as the RFC 3339 instant the server reads. */
export function localToIso(value: string): string | undefined {
  if (!value) return undefined;
  const at = new Date(value);
  return Number.isNaN(at.getTime()) ? undefined : at.toISOString();
}

/** Where a span's row goes: its run, at the span. */
export const spanHref = (span: SecopsSpan) => `/app/runs/${span.run_id}?span=${span.id}`;
