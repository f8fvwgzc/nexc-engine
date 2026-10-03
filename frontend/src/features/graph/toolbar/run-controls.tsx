import { ChevronDownIcon, PlayIcon, SquareIcon } from 'lucide-react';
import { useState } from 'react';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Label } from '@/components/ui/label';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Switch } from '@/components/ui/switch';
import { useCancelRun } from '@/features/runs/hooks/use-cancel-run';
import { isRunActive } from '@/schemas/run';
import { useGraphStore } from '@/stores/graph-store';

import { useStartRun } from '../hooks/use-graph-mutations';

export function RunControls({ graphId, disabled }: { graphId: string; disabled: boolean }) {
  const run = useGraphStore((s) => s.run);
  const [force, setForce] = useState(false);
  const startRun = useStartRun(graphId);
  const cancelRun = useCancelRun();

  if (run && isRunActive(run.status)) {
    return (
      <AnimatedButton
        size="sm"
        variant="destructive"
        loading={cancelRun.isPending}
        onClick={() => cancelRun.mutate(run.id)}
      >
        <SquareIcon className="fill-current" />
        Cancel
      </AnimatedButton>
    );
  }

  return (
    <ButtonGroup>
      <AnimatedButton
        size="sm"
        disabled={disabled}
        loading={startRun.isPending}
        onClick={() => startRun.mutate({ force })}
      >
        <PlayIcon className="fill-current" />
        {force ? 'Force run' : 'Run'}
      </AnimatedButton>
      <Popover>
        <PopoverTrigger asChild>
          <Button size="icon-sm" disabled={disabled} aria-label="Run options">
            <ChevronDownIcon />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-72">
          <div className="flex items-start justify-between gap-4">
            <div className="space-y-1">
              <Label htmlFor="force-run">Force re-run</Label>
              <p className="text-xs text-muted-foreground">
                Ignore the result cache and execute every node again.
              </p>
            </div>
            <Switch id="force-run" checked={force} onCheckedChange={setForce} />
          </div>
        </PopoverContent>
      </Popover>
    </ButtonGroup>
  );
}
