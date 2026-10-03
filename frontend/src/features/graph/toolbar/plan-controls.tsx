import { CheckIcon, SparklesIcon, XIcon } from 'lucide-react';
import { useState } from 'react';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { Spinner } from '@/components/ui/spinner';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Textarea } from '@/components/ui/textarea';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useGraphStore } from '@/stores/graph-store';

import { useApplyPlan, useRequestPlan } from '../hooks/use-graph-mutations';

function RequestPlanPopover({ graphId }: { graphId: string }) {
  const [open, setOpen] = useState(false);
  const [instructions, setInstructions] = useState('');
  const requestPlan = useRequestPlan(graphId);

  const submit = () => {
    requestPlan.mutate(instructions.trim() || undefined, { onSuccess: () => setOpen(false) });
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" aria-label="Request plan">
          <SparklesIcon className="text-brand" />
          <span className="hidden sm:inline">Plan</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80 space-y-3">
        <div className="space-y-1">
          <p className="text-sm font-medium">Refine this graph with the LLM</p>
          <p className="text-xs text-muted-foreground">
            Proposed nodes stream onto the canvas as dashed ghosts. Nothing changes until you apply.
          </p>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="plan-instructions">Instructions (optional)</Label>
          <Textarea
            id="plan-instructions"
            rows={3}
            value={instructions}
            placeholder="e.g. Split research into sources, outline and citations"
            onChange={(e) => setInstructions(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submit();
            }}
          />
        </div>
        <AnimatedButton
          glow
          className="w-full"
          loading={requestPlan.isPending}
          loadingText="Starting…"
          onClick={submit}
        >
          <SparklesIcon />
          Request plan
        </AnimatedButton>
      </PopoverContent>
    </Popover>
  );
}

interface PlanControlsProps {
  graphId: string;
  onApplied: () => void;
}

export function PlanControls({ graphId, onApplied }: PlanControlsProps) {
  const plan = useGraphStore((s) => s.plan);
  const clearPlan = useGraphStore((s) => s.clearPlan);
  const applyPlan = useApplyPlan(graphId);

  if (!plan || plan.status === 'failed' || plan.status === 'applied') {
    return <RequestPlanPopover graphId={graphId} />;
  }

  const discard = (
    <Button variant="ghost" size="icon-sm" aria-label="Discard plan" onClick={clearPlan}>
      <XIcon />
    </Button>
  );

  if (plan.status !== 'ready') {
    return (
      <div className="flex items-center gap-1" role="status" aria-live="polite">
        <span className="flex h-7 items-center gap-2 rounded-md bg-brand/10 px-2.5 text-xs font-medium text-brand">
          <Spinner className="size-3.5" />
          Planning{plan.nodes.length > 0 ? ` · ${plan.nodes.length}` : '…'}
        </span>
        {discard}
      </div>
    );
  }

  return (
    <div className="flex items-center gap-1">
      <Tooltip>
        <TooltipTrigger asChild>
          <AnimatedButton
            size="sm"
            glow
            loading={applyPlan.isPending}
            onClick={() => plan.id && applyPlan.mutate(plan.id, { onSuccess: onApplied })}
          >
            <CheckIcon />
            Apply plan
            <span className="hidden opacity-80 sm:inline">
              ({plan.nodes.length} nodes · {plan.edges.length} edges)
            </span>
          </AnimatedButton>
        </TooltipTrigger>
        {plan.summary && <TooltipContent className="max-w-xs">{plan.summary}</TooltipContent>}
      </Tooltip>
      {discard}
    </div>
  );
}
