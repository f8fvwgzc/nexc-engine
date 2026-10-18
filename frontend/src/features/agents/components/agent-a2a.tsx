import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useId } from 'react';
import { Link } from 'react-router-dom';
import { toast } from 'sonner';

import { CopyButton } from '@/components/custom-ui/copy-button';
import { Badge } from '@/components/ui/badge';
import { Label } from '@/components/ui/label';
import { Skeleton } from '@/components/ui/skeleton';
import { Switch } from '@/components/ui/switch';
import { a2aAddresses } from '@/features/a2a/api';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import {
  A2A_TASK_STATES,
  displayPrincipal,
  type A2aTask,
  type A2aTaskState,
  type Agent,
} from '@/schemas/agent';

import { agentA2aTasksQuery, setAgentA2aExposed } from '../api';

const STATE_LABEL: Record<A2aTaskState, string> = {
  submitted: 'submitted',
  working: 'working',
  completed: 'completed',
  canceled: 'canceled',
  failed: 'failed',
};

/** The state in words; one the console does not know is shown as it came. */
function stateLabel(state: string): string {
  return (A2A_TASK_STATES as readonly string[]).includes(state)
    ? STATE_LABEL[state as A2aTaskState]
    : state;
}

function Address({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="flex items-center gap-1">
        <code className="min-w-0 flex-1 truncate rounded bg-muted px-1 py-0.5 font-mono text-xs">
          {value}
        </code>
        <CopyButton value={value} label={`Copy ${label.toLowerCase()}`} />
      </dd>
    </div>
  );
}

function TaskRow({ task }: { task: A2aTask }) {
  const refused = task.refused_rule !== null;
  return (
    <li>
      <Link
        to={`/app/runs/${task.run_id}`}
        className="flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-1.5 text-xs outline-none hover:bg-muted/60 focus-visible:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset"
      >
        <span className="min-w-0 flex-1 truncate" title={task.title}>
          {task.title}
        </span>
        <span className="text-muted-foreground" title={task.caller_principal}>
          from {displayPrincipal(task.caller_principal)}
        </span>
        {task.hop > 0 && (
          <Badge variant="outline" className="h-5 text-[10px]" title={task.chain.join(' → ')}>
            hop {task.hop}
          </Badge>
        )}
        <Badge
          variant={refused || task.state === 'failed' ? 'destructive' : 'outline'}
          className="h-5 text-[10px]"
          title={task.refused_reason ?? undefined}
        >
          {refused ? `refused · ${task.refused_rule}` : stateLabel(task.state)}
        </Badge>
        <time dateTime={task.created_at} className="shrink-0 text-muted-foreground tabular-nums">
          {formatRelative(task.created_at)}
        </time>
      </Link>
    </li>
  );
}

interface AgentA2aProps {
  agent: Agent;
  /** Whether the viewer may publish and withdraw (workspace admins). */
  admin: boolean;
}

/**
 * The agent over A2A (CONTRACT §10): the switch that publishes it (admins), the addresses of
 * its card and endpoint with a copy, and the tasks it received from counterparts, each
 * opening the run that answered it.
 */
export function AgentA2a({ agent, admin }: AgentA2aProps) {
  const queryClient = useQueryClient();
  const switchId = useId();
  const external = agent.runtime === 'external';
  const tasks = useQuery({ ...agentA2aTasksQuery(agent.id), enabled: agent.a2a_exposed });
  const expose = useMutation({
    mutationFn: (exposed: boolean) => setAgentA2aExposed(agent.id, exposed),
    onSuccess: (next) => {
      toast.success(
        next.a2a_exposed
          ? `${next.name} is published over A2A`
          : `${next.name} is withdrawn from A2A`,
      );
      queryClient.setQueriesData<Agent[]>({ queryKey: qk.agents.all }, (list) =>
        list?.map((a) => (a.id === next.id ? next : a)),
      );
      void queryClient.invalidateQueries({ queryKey: qk.agents.card(agent.id) });
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const addresses = a2aAddresses(agent.principal_id);

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between gap-3">
        <div className="space-y-1">
          <Label htmlFor={switchId} className="text-sm">
            Published over A2A
          </Label>
          <p className="text-xs text-muted-foreground">
            {external
              ? 'An external agent runs elsewhere and cannot take A2A tasks here.'
              : 'Registered counterparts may send this agent tasks; each is a run of its own, on the record with who called and the decision that admitted it.'}
          </p>
        </div>
        <Switch
          id={switchId}
          checked={agent.a2a_exposed}
          disabled={!admin || external || expose.isPending}
          onCheckedChange={(checked) => expose.mutate(checked)}
          aria-label="Published over A2A"
        />
      </div>
      {!admin && (
        <p className="text-xs text-muted-foreground">Workspace admins publish and withdraw.</p>
      )}
      {agent.a2a_exposed && (
        <dl className="space-y-2">
          <Address label="Card URL" value={addresses.card} />
          <Address label="Well-known card" value={addresses.wellKnown} />
          <Address label="JSON-RPC endpoint" value={addresses.endpoint} />
        </dl>
      )}
      <section className="space-y-1.5">
        <h3 className="text-xs font-medium text-muted-foreground">Inbound tasks</h3>
        {!agent.a2a_exposed ? (
          <p className="text-xs text-muted-foreground">
            Publish the agent and the tasks it receives are listed here.
          </p>
        ) : tasks.isPending ? (
          <Skeleton className="h-12 rounded-lg" />
        ) : tasks.error ? (
          <p role="alert" className="text-xs text-destructive">
            {errorMessage(tasks.error)}
          </p>
        ) : tasks.data.length === 0 ? (
          <p className="text-xs text-muted-foreground">No task received yet.</p>
        ) : (
          <ol
            className={cn('divide-y rounded-lg border', tasks.isFetching && 'opacity-90')}
            aria-label="Inbound tasks"
          >
            {tasks.data.map((task) => (
              <TaskRow key={task.id} task={task} />
            ))}
          </ol>
        )}
      </section>
    </div>
  );
}
