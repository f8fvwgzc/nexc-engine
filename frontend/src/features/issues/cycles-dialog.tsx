import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { PlusIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import { cycleLabel, type Cycle } from '@/schemas/cycle';
import type { Team, Workspace } from '@/schemas/workspace';

import { createCycle, cyclesQuery, deleteCycle } from './api';

const STATUS_LABEL: Record<Cycle['status'], string> = {
  upcoming: 'Upcoming',
  active: 'Active',
  completed: 'Completed',
};

const dayFormat = new Intl.DateTimeFormat('en', {
  month: 'short',
  day: 'numeric',
  timeZone: 'UTC',
});
/** `2026-10-06` as "Oct 6", without shifting it into the viewer's time zone. */
const formatDay = (day: string) => dayFormat.format(new Date(`${day}T00:00:00Z`));

/** `day` plus `days`, as `YYYY-MM-DD`. */
function addDays(day: string, days: number): string {
  const date = new Date(`${day}T00:00:00Z`);
  date.setUTCDate(date.getUTCDate() + days);
  return date.toISOString().slice(0, 10);
}

/**
 * The cycles of a team, latest first, each with how many of its issues are closed. Team owners
 * and workspace admins plan and delete cycles; a new one defaults to the two weeks after the
 * latest.
 */
export function CyclesDialog({
  workspace,
  team,
  editable,
  open,
  onClose,
}: {
  workspace: Workspace;
  team: Team;
  editable: boolean;
  open: boolean;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const { data: cycles = [] } = useQuery({ ...cyclesQuery(workspace.id, team.id), enabled: open });
  const latestEnd = cycles.reduce<string | null>(
    (max, c) => (max === null || c.ends_on > max ? c.ends_on : max),
    null,
  );
  const today = new Date().toISOString().slice(0, 10);
  const suggestedStart = latestEnd && latestEnd >= today ? addDays(latestEnd, 1) : today;
  const [name, setName] = useState('');
  // Empty until edited, so the suggestion follows the list as cycles are added.
  const [start, setStart] = useState('');
  const [end, setEnd] = useState('');
  const startsOn = start || suggestedStart;
  const endsOn = end || addDays(startsOn, 13);
  const reload = () => queryClient.invalidateQueries({ queryKey: qk.issues.all });
  const add = useMutation({
    mutationFn: () =>
      createCycle(workspace.id, team.id, {
        name: name.trim(),
        starts_on: startsOn,
        ends_on: endsOn,
      }),
    meta: { errorToast: false },
    onSuccess: () => {
      setName('');
      setStart('');
      setEnd('');
      void reload();
    },
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteCycle(workspace.id, team.id, id),
    meta: { errorToast: false },
    onSuccess: reload,
  });

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Cycles · {team.name}</DialogTitle>
          <DialogDescription>
            Time boxes {team.key} plans its issues in. Cycles do not overlap; deleting one leaves
            its issues in place.
          </DialogDescription>
        </DialogHeader>
        {cycles.length === 0 ? (
          <p className="text-[13px] text-muted-foreground">No cycles planned yet.</p>
        ) : (
          <ul className="max-h-72 divide-y overflow-y-auto rounded-md border">
            {cycles.map((cycle) => (
              <li key={cycle.id} className="flex items-center gap-2.5 px-3 py-2 text-[13px]">
                <span className="min-w-0 flex-1">
                  <span className="font-medium">{cycleLabel(cycle)}</span>{' '}
                  <span className="text-muted-foreground">
                    {formatDay(cycle.starts_on)} – {formatDay(cycle.ends_on)}
                  </span>
                </span>
                <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
                  {cycle.closed_count}/{cycle.issue_count} closed
                </span>
                <Badge variant={cycle.status === 'active' ? 'default' : 'outline'}>
                  {STATUS_LABEL[cycle.status]}
                </Badge>
                {editable && (
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-7 text-muted-foreground hover:text-destructive"
                    aria-label={`Delete ${cycleLabel(cycle)}`}
                    disabled={remove.isPending}
                    onClick={() => remove.mutate(cycle.id)}
                  >
                    <Trash2Icon className="size-3.5" />
                  </Button>
                )}
              </li>
            ))}
          </ul>
        )}
        {editable && (
          <form
            className="flex flex-wrap items-end gap-2 border-t pt-3"
            onSubmit={(e) => {
              e.preventDefault();
              add.mutate();
            }}
          >
            <label className="flex-1 space-y-1 text-xs text-muted-foreground">
              Name (optional)
              <Input
                value={name}
                maxLength={60}
                placeholder="e.g. Launch"
                className="h-8 text-[13px] text-foreground"
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            <label className="space-y-1 text-xs text-muted-foreground">
              First day
              <Input
                type="date"
                required
                value={startsOn}
                className="h-8 text-[13px] text-foreground"
                onChange={(e) => setStart(e.target.value)}
              />
            </label>
            <label className="space-y-1 text-xs text-muted-foreground">
              Last day
              <Input
                type="date"
                required
                value={endsOn}
                min={startsOn}
                className="h-8 text-[13px] text-foreground"
                onChange={(e) => setEnd(e.target.value)}
              />
            </label>
            <Button type="submit" size="sm" disabled={add.isPending}>
              <PlusIcon />
              Plan cycle
            </Button>
          </form>
        )}
        {(add.error ?? remove.error) && (
          <p role="alert" className="text-sm text-destructive">
            {errorMessage(add.error ?? remove.error)}
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}
