import { PauseIcon, PlayIcon, RadioIcon, SirenIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';
import { Link } from 'react-router-dom';

import { Button } from '@/components/ui/button';
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select';
import { SeverityBadge } from '@/features/policies/components/severity-badge';
import { SPAN_KIND_META } from '@/features/runs/span-kinds';
import { describeSpan } from '@/features/runs/trace-tree';
import type { ConnectionState } from '@/lib/realtime/connection-state';
import { cn } from '@/lib/utils';
import { displayPrincipal, type Agent } from '@/schemas/agent';
import { SPAN_KINDS } from '@/schemas/trace';

import { feedLink, filterFeed, type FeedEvent, type KindFilter } from '../feed';
import { useSecopsFeed } from '../hooks/use-secops-feed';
import { Panel, PanelEmpty } from './console-panel';

const STATE_LABEL: Record<ConnectionState, string> = {
  connecting: 'connecting',
  open: 'live',
  reconnecting: 'reconnecting',
  closed: 'off',
};

const time = new Intl.DateTimeFormat('en', {
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
});

function Line({ event }: { event: FeedEvent }) {
  const link = feedLink(event);
  const at = <time dateTime={event.at}>{time.format(new Date(event.at))}</time>;
  if (event.kind === 'alert') {
    const { alert } = event;
    const body = (
      <>
        <SirenIcon className="size-3.5 shrink-0 text-destructive" aria-label="Alert" />
        <span className="shrink-0 text-muted-foreground tabular-nums">{at}</span>
        <SeverityBadge severity={alert.severity} />
        <span className="min-w-0 flex-1 truncate">
          <span className="font-medium">{displayPrincipal(alert.principal)}</span> · {alert.title}
        </span>
      </>
    );
    return (
      <li>
        {link ? (
          <Link to={link} className={ROW}>
            {body}
          </Link>
        ) : (
          <div className={ROW}>{body}</div>
        )}
      </li>
    );
  }
  const { span } = event;
  const meta = SPAN_KIND_META[span.kind];
  const Icon = meta.icon;
  const error = span.status === 'error';
  const detail = describeSpan(span);
  return (
    <li>
      <Link to={link ?? '#'} className={ROW}>
        <Icon
          className={cn('size-3.5 shrink-0 text-muted-foreground', error && 'text-destructive')}
          aria-label={meta.label}
        />
        <span className="shrink-0 text-muted-foreground tabular-nums">{at}</span>
        <span className="shrink-0 font-medium" title={span.principal}>
          {displayPrincipal(span.principal)}
        </span>
        <span className={cn('truncate font-mono', error && 'text-destructive')}>{span.name}</span>
        {detail && (
          <span className="hidden truncate text-muted-foreground sm:inline">{detail}</span>
        )}
      </Link>
    </li>
  );
}

const ROW =
  'flex items-center gap-2 px-3 py-1 text-xs outline-none hover:bg-muted/60 focus-visible:bg-muted/60';

interface LiveFeedPanelProps {
  workspaceId: string;
  agents: Agent[];
  className?: string;
}

/**
 * Every span and alert of the workspace as it happens, newest first. The feed can be paused
 * (lines are held back, not lost) and narrowed to a kind of action or one agent; a line opens
 * the run at that span.
 */
export function LiveFeedPanel({ workspaceId, agents, className }: LiveFeedPanelProps) {
  const [paused, setPaused] = useState(false);
  const [kind, setKind] = useState<KindFilter>('all');
  const [agent, setAgent] = useState('all');
  const feed = useSecopsFeed(workspaceId, paused);
  const shown = filterFeed(feed.events, kind, agent);
  const open = feed.state === 'open';
  return (
    <Panel
      id="secops-feed"
      title="Live feed"
      icon={RadioIcon}
      count={feed.events.length}
      className={className}
      bodyClassName="p-0"
      hint={
        <span className="flex items-center gap-1.5">
          <span
            aria-hidden
            className={cn(
              'size-1.5 rounded-full',
              open
                ? 'bg-[var(--status-succeeded)] motion-safe:animate-pulse'
                : 'bg-muted-foreground/50',
            )}
          />
          <span role="status">
            {paused
              ? `paused${feed.held > 0 ? ` · ${feed.held} held` : ''}`
              : STATE_LABEL[feed.state]}
          </span>
        </span>
      }
      actions={
        <>
          <NativeSelect
            aria-label="Kind"
            size="sm"
            value={kind}
            onChange={(e) => setKind(e.target.value as KindFilter)}
          >
            <NativeSelectOption value="all">Everything</NativeSelectOption>
            <NativeSelectOption value="alert">Alerts</NativeSelectOption>
            {SPAN_KINDS.map((k) => (
              <NativeSelectOption key={k} value={k}>
                {SPAN_KIND_META[k].label}
              </NativeSelectOption>
            ))}
          </NativeSelect>
          <NativeSelect
            aria-label="Agent"
            size="sm"
            value={agent}
            onChange={(e) => setAgent(e.target.value)}
          >
            <NativeSelectOption value="all">Every agent</NativeSelectOption>
            {agents.map((a) => (
              <NativeSelectOption key={a.id} value={a.id}>
                {a.name}
              </NativeSelectOption>
            ))}
          </NativeSelect>
          <Button
            size="icon-sm"
            variant="outline"
            aria-pressed={paused}
            aria-label={paused ? 'Resume the feed' : 'Pause the feed'}
            onClick={() => setPaused((p) => !p)}
          >
            {paused ? <PlayIcon /> : <PauseIcon />}
          </Button>
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label="Clear the feed"
            disabled={feed.events.length === 0}
            onClick={feed.clear}
          >
            <Trash2Icon />
          </Button>
        </>
      }
    >
      {shown.length === 0 ? (
        <PanelEmpty>
          {feed.events.length === 0
            ? 'Spans and alerts appear here as the agents work.'
            : 'Nothing matches the filter.'}
        </PanelEmpty>
      ) : (
        <ul
          className="max-h-[22rem] divide-y overflow-y-auto"
          aria-live={paused ? 'off' : 'polite'}
        >
          {shown.map((event) => (
            <Line key={`${event.kind}:${event.id}`} event={event} />
          ))}
        </ul>
      )}
    </Panel>
  );
}
