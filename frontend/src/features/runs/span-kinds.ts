import {
  BrainCircuitIcon,
  DatabaseIcon,
  GitForkIcon,
  GlobeIcon,
  ShieldIcon,
  UserCheckIcon,
  WrenchIcon,
  type LucideIcon,
} from 'lucide-react';

import type { SpanKind } from '@/schemas/trace';

interface SpanKindMeta {
  label: string;
  icon: LucideIcon;
  /** Tailwind class of the span's bar in the waterfall. */
  bar: string;
}

/** How each kind of span is shown (CONTRACT §4). */
export const SPAN_KIND_META: Record<SpanKind, SpanKindMeta> = {
  llm_call: { label: 'Model call', icon: BrainCircuitIcon, bar: 'bg-brand/70' },
  tool_call: { label: 'Tool call', icon: WrenchIcon, bar: 'bg-[var(--status-running)]' },
  spawn: { label: 'Sub-agent', icon: GitForkIcon, bar: 'bg-[var(--status-cached)]' },
  data_read: { label: 'Context read', icon: DatabaseIcon, bar: 'bg-[var(--status-queued)]' },
  egress: { label: 'External call', icon: GlobeIcon, bar: 'bg-[var(--status-skipped)]' },
  policy: { label: 'Policy decision', icon: ShieldIcon, bar: 'bg-[var(--status-succeeded)]' },
  approval: { label: 'Approval', icon: UserCheckIcon, bar: 'bg-[var(--status-succeeded)]' },
};
