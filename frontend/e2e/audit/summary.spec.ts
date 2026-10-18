/** Folds the audit's per-page records into `e2e/.audit/<phase>/summary.json` (runs after it). */
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { test } from '@playwright/test';

import type { AxeFinding, LayoutReport } from '../support/page';

const PHASE = process.env.AUDIT_PHASE ?? 'current';
const AUDIT_DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '.audit', PHASE);

interface AuditRecord {
  route: string;
  mode: string;
  theme: string;
  width: number;
  unhandled: string[];
  stuckLoading: number;
  duplicates: string[];
  requests: number;
  layout: LayoutReport;
  fullLayout: LayoutReport;
  axe: AxeFinding[];
  iconOnly?: { label: string; title: boolean; tooltip: boolean; menu: boolean; html: string }[];
  truncation?: string[];
  headings?: { text: string; inset: number; top: number; size: string }[];
  empty?: { title: string; icon: boolean; description: boolean; action: boolean }[];
  focus?: string[];
  polls?: string[];
  refetchOnFocus?: string[];
  errors: string[];
}

test('summarise the audit', () => {
  const dir = join(AUDIT_DIR, 'records');
  test.skip(!existsSync(dir), 'no audit records');
  const records = readdirSync(dir).map(
    (file) => JSON.parse(readFileSync(join(dir, file), 'utf8')) as AuditRecord,
  );
  const at = (r: AuditRecord) => `${r.mode}/${r.theme}/${r.width}`;
  const byRoute = new Map<string, AuditRecord[]>();
  for (const r of records) byRoute.set(r.route, [...(byRoute.get(r.route) ?? []), r]);

  const summary = [...byRoute.entries()].sort().map(([route, rs]) => {
    const axe = new Map<
      string,
      { impact: string; help: string; where: string[]; nodes: string[] }
    >();
    for (const r of rs) {
      for (const v of r.axe) {
        const entry = axe.get(v.id) ?? {
          impact: v.impact,
          help: v.help,
          where: [],
          nodes: v.nodes,
        };
        entry.where.push(at(r));
        axe.set(v.id, entry);
      }
    }
    const primary = rs.filter((r) => r.iconOnly);
    return {
      route,
      unhandled: [...new Set(rs.flatMap((r) => r.unhandled))],
      errors: [...new Set(rs.flatMap((r) => r.errors))].slice(0, 5),
      stuckLoading: rs.filter((r) => r.stuckLoading > 0).map(at),
      horizontalScroll: rs
        .filter((r) => r.layout.docOverflow > 0 || r.layout.mainOverflow > 0)
        .map((r) => `${at(r)}: doc +${r.layout.docOverflow}px, main +${r.layout.mainOverflow}px`),
      overlaps: rs
        .filter((r) => r.fullLayout.overlaps.length > 0)
        .map(
          (r) =>
            `${at(r)}: ${r.fullLayout.overlaps.map((o) => `${o.a} × ${o.b} (${o.area}px²)`).join('; ')}`,
        ),
      axe: Object.fromEntries(axe),
      unnamedIconControls: [
        ...new Set(
          primary.flatMap((r) =>
            (r.iconOnly ?? []).filter((c) => !c.label).map((c) => `${at(r)}: ${c.html}`),
          ),
        ),
      ],
      iconControlsWithoutTooltip: [
        ...new Set(
          primary.flatMap((r) =>
            (r.iconOnly ?? [])
              .filter((c) => c.label && !c.title && !c.tooltip && !c.menu)
              .map((c) => c.label),
          ),
        ),
      ],
      truncation: [
        ...new Set(primary.flatMap((r) => (r.truncation ?? []).map((t) => `${r.width}: ${t}`))),
      ],
      headings: primary.map(
        (r) =>
          `${at(r)}: ${(r.headings ?? []).map((h) => `"${h.text}" inset ${h.inset}px ${h.size}`).join(', ') || 'no h1'}`,
      ),
      emptyStates: primary
        .filter((r) => r.mode === 'empty')
        .map(
          (r) =>
            `${r.width}: ${(r.empty ?? []).map((e) => `${e.title} (icon ${e.icon}, text ${e.description}, action ${e.action})`).join('; ') || 'none'}`,
        ),
      focus: [...new Set(rs.flatMap((r) => r.focus ?? []))],
      requests: Math.max(...rs.map((r) => r.requests)),
      duplicates: [...new Set(rs.flatMap((r) => r.duplicates))],
      polls: [...new Set(rs.flatMap((r) => r.polls ?? []))],
      refetchOnFocus: [...new Set(rs.flatMap((r) => r.refetchOnFocus ?? []))],
    };
  });
  writeFileSync(join(AUDIT_DIR, 'summary.json'), JSON.stringify(summary, null, 2));

  // Loading and error states, by route.
  const statesDir = join(AUDIT_DIR, 'states');
  if (existsSync(statesDir)) {
    const states: Record<string, Record<string, unknown>> = {};
    for (const file of readdirSync(statesDir)) {
      const r = JSON.parse(readFileSync(join(statesDir, file), 'utf8')) as {
        route: string;
        mode: string;
        width: number;
        state: unknown;
      };
      (states[r.route] ??= {})[`${r.mode}/${r.width}`] = r.state;
    }
    writeFileSync(join(AUDIT_DIR, 'states.json'), JSON.stringify(states, null, 2));
  }
});
