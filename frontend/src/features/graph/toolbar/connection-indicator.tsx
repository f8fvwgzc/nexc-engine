import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { ConnectionState } from '@/lib/realtime/connection-state';
import { cn } from '@/lib/utils';

const LABEL: Record<ConnectionState, string> = {
  connecting: 'Connecting…',
  open: 'Live',
  reconnecting: 'Reconnecting…',
  closed: 'Offline',
};

/** Combined health of the SSE (runs/plans) and WebSocket (collaboration) channels. */
export function ConnectionIndicator({ sse, ws }: { sse: ConnectionState; ws: ConnectionState }) {
  const live = sse === 'open' && ws === 'open';
  const label = live ? 'Live' : LABEL[sse === 'open' ? ws : sse];
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          role="status"
          aria-label={`Realtime: ${label}`}
          className="flex h-7 items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground"
        >
          <span
            className={cn(
              'size-2 rounded-full',
              live ? 'bg-status-succeeded' : 'bg-status-queued motion-safe:animate-pulse',
            )}
          />
          <span className="hidden md:inline">{label}</span>
        </span>
      </TooltipTrigger>
      <TooltipContent>
        Events: {LABEL[sse]} · Collaboration: {LABEL[ws]}
      </TooltipContent>
    </Tooltip>
  );
}
