import type { AlertDestination } from '@/schemas/alert-routing';
import { SEVERITY_META, type AlertKind } from '@/schemas/policy';

/** The alert kinds as the console names them. */
export const ALERT_KIND_LABEL: Record<AlertKind, string> = {
  dlp: 'Data leaving',
  risk: 'Behaviour',
  injection: 'Injection',
  policy: 'Policy',
};

/** What a destination sends, in a line: "data leaving, injection from medium up, approvals too". */
export function describeFilters(d: AlertDestination): string {
  const kinds =
    d.kinds.length === 0
      ? 'every kind'
      : d.kinds.map((k) => ALERT_KIND_LABEL[k].toLowerCase()).join(', ');
  const from =
    d.min_severity === 'low' ? '' : ` from ${SEVERITY_META[d.min_severity].label.toLowerCase()} up`;
  return `${kinds}${from}${d.include_approvals ? ', approvals too' : ''}`;
}
