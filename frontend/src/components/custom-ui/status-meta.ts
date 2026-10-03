import {
  CircleCheckIcon,
  CircleDashedIcon,
  CircleDotIcon,
  CircleSlashIcon,
  CircleXIcon,
  ClockIcon,
  DatabaseZapIcon,
  LoaderCircleIcon,
  type LucideIcon,
} from 'lucide-react';

import type { NodeStatus } from '@/schemas/graph';
import type { RunStatus } from '@/schemas/run';

/** `cached` is not a NodeStatus in the contract — it is `succeeded` + `cached: true`. */
export type DisplayStatus = NodeStatus | RunStatus | 'cached';

interface StatusMeta {
  label: string;
  icon: LucideIcon;
  /** CSS custom property holding the status color (see styles/index.css). */
  color: string;
}

export const STATUS_META: Record<DisplayStatus, StatusMeta> = {
  idle: { label: 'Idle', icon: CircleDashedIcon, color: 'var(--status-idle)' },
  queued: { label: 'Queued', icon: ClockIcon, color: 'var(--status-queued)' },
  running: { label: 'Running', icon: LoaderCircleIcon, color: 'var(--status-running)' },
  succeeded: { label: 'Succeeded', icon: CircleCheckIcon, color: 'var(--status-succeeded)' },
  failed: { label: 'Failed', icon: CircleXIcon, color: 'var(--status-failed)' },
  skipped: { label: 'Skipped', icon: CircleDotIcon, color: 'var(--status-skipped)' },
  cancelled: { label: 'Cancelled', icon: CircleSlashIcon, color: 'var(--status-cancelled)' },
  cached: { label: 'Cached', icon: DatabaseZapIcon, color: 'var(--status-cached)' },
};

export function displayStatus(status: NodeStatus, cached: boolean): DisplayStatus {
  return status === 'succeeded' && cached ? 'cached' : status;
}
