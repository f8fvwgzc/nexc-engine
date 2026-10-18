import { SEVERITY_META, type Severity } from '@/schemas/policy';

/** A coloured pill for a severity. */
export function SeverityBadge({ severity }: { severity: Severity }) {
  const meta = SEVERITY_META[severity];
  return (
    <span
      style={{ '--badge-color': meta.color } as React.CSSProperties}
      className="inline-flex h-5 items-center gap-1.5 rounded-full bg-[color-mix(in_oklch,var(--badge-color)_12%,transparent)] px-2 text-[11px] font-medium text-badge"
    >
      <span className="size-1.5 rounded-full bg-[var(--badge-color)]" />
      {meta.label}
    </span>
  );
}
