import type { ResolvedPolicy } from '@/schemas/policy';

const list = (items: string[] | null, any: string, none: string) =>
  items === null ? any : items.length === 0 ? none : items.join(', ');
const limit = (value: number | null, unit = '') =>
  value === null ? 'no limit' : `${value}${unit}`;

/** Every rule of the effective policy as a row: the path, the value, the level that set it. */
export function ruleRows(resolved: ResolvedPolicy): { path: string; value: string }[] {
  const { capabilities: c, rules: r } = resolved;
  const rows: { path: string; value: string }[] = [
    { path: 'tools.allow', value: list(c.tools, 'every tool', 'none') },
    { path: 'tools.deny', value: list(r.tools_denied, '', 'none') },
    { path: 'egress.allow', value: list(c.egress_hosts, 'any host', 'none') },
    { path: 'egress.deny', value: list(r.egress_denied, '', 'none') },
    { path: 'models.allow', value: list(c.models, 'any model', 'none') },
    { path: 'models.deny', value: list(r.models_denied, '', 'none') },
    { path: 'labels', value: c.data.labels.join(', ') || 'none' },
    { path: 'limits.tokens_per_run', value: limit(r.limits.tokens_per_run, ' tokens') },
    { path: 'limits.tokens_per_day', value: limit(r.limits.tokens_per_day, ' tokens') },
    { path: 'limits.cost_usd_per_day', value: limit(r.limits.cost_usd_per_day, ' USD') },
    {
      path: 'limits.tool_calls_per_minute',
      value: limit(r.limits.tool_calls_per_minute, ' per minute'),
    },
    { path: 'limits.runs_per_hour', value: limit(r.limits.runs_per_hour, ' per hour') },
    { path: 'limits.max_depth', value: `${c.delegation.max_depth} level(s)` },
    { path: 'limits.max_fan_out', value: `${c.delegation.max_fan_out} per agent` },
    {
      path: 'time_windows',
      value:
        r.time_windows.length === 0
          ? 'any time'
          : r.time_windows
              .map((set) =>
                set.length === 0
                  ? 'never'
                  : set.map((w) => `${w.days.join('')} ${w.start}–${w.end}`).join(', '),
              )
              .join(' ∩ '),
    },
    { path: 'require_approval.tools', value: list(r.require_approval.tools, '', 'none') },
    { path: 'require_approval.labels', value: list(r.require_approval.labels, '', 'none') },
    {
      path: 'require_approval.approvers',
      value:
        r.require_approval.approvers.length === 0
          ? 'every admin'
          : `${r.require_approval.approvers.length} named`,
    },
    { path: 'require_approval.timeout_s', value: `${r.require_approval.timeout_s} s` },
    { path: 'alerts.risk_score', value: limit(r.alerts.risk_score) },
    { path: 'alerts.quarantine_risk_score', value: limit(r.alerts.quarantine_risk_score) },
    { path: 'alerts.denials_per_run', value: limit(r.alerts.denials_per_run) },
    { path: 'alerts.novelty', value: limit(r.alerts.novelty) },
    { path: 'dlp.secrets', value: r.dlp.secrets },
    { path: 'dlp.pii.enabled', value: r.dlp.pii.enabled ? 'on' : 'off' },
    { path: 'dlp.pii.kinds', value: r.dlp.pii.kinds.join(', ') || 'none' },
    { path: 'dlp.pii.keywords', value: r.dlp.pii.keywords.join(', ') || 'none' },
    { path: 'dlp.pii.action', value: r.dlp.pii.action },
  ];
  for (const [label, outbound] of Object.entries(r.dlp.labels)) {
    if (!outbound) continue;
    rows.push({
      path: `dlp.labels.${label}.artifacts`,
      value: outbound.artifacts ? 'may leave as an artifact' : 'stays in',
    });
    rows.push({
      path: `dlp.labels.${label}.egress_hosts`,
      value: list(outbound.egress_hosts, 'any allowed host', 'no host'),
    });
  }
  rows.push({ path: 'injection.action', value: r.injection.action });
  rows.push({ path: 'injection.model_check', value: r.injection.model_check ? 'on' : 'off' });
  rows.push({
    path: 'evidence.required',
    value: r.evidence.required ? 'on' : 'off (recorded only)',
  });
  rows.push({
    path: 'evidence.min_coverage',
    value: `${Math.round(r.evidence.min_coverage * 100)}% of claims cited`,
  });
  rows.push({ path: 'evidence.action', value: r.evidence.action });
  return rows;
}
