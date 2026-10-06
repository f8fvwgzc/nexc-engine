import { GlassCard } from '@/components/custom-ui/glass-card';
import { Switch } from '@/components/ui/switch';

function Line({ className, arrow = false }: { className: string; arrow?: boolean }) {
  return (
    <svg width="34" height="10" viewBox="0 0 34 10" aria-hidden className="shrink-0">
      <path d={arrow ? 'M1 5 H27' : 'M1 5 H33'} fill="none" className={className} />
      {arrow && <path d="M26 1.5 L33 5 L26 8.5 Z" className="fill-(--canvas-edge) stroke-none" />}
    </svg>
  );
}

/** What the lines on the canvas mean, and the switch for dependency suggestions. */
export function CanvasLegend({
  suggestionCount,
  showSuggestions,
  onShowSuggestions,
}: {
  suggestionCount: number;
  showSuggestions: boolean;
  onShowSuggestions: (show: boolean) => void;
}) {
  return (
    <GlassCard
      role="group"
      aria-label="Canvas legend"
      className="pointer-events-auto space-y-1.5 px-3 py-2 text-xs text-muted-foreground"
    >
      <div className="flex items-center gap-2">
        <Line arrow className="stroke-(--canvas-edge) stroke-[1.6]" />
        Runs first → runs after (blocking)
      </div>
      <div className="flex items-center gap-2">
        <Line className="stroke-(--canvas-edge) stroke-[1.6] [stroke-dasharray:5_5]" />
        Related, no ordering
      </div>
      <label className="flex cursor-pointer items-center gap-2">
        <Line className="stroke-brand stroke-[1.6] [stroke-dasharray:6_6]" />
        <span className="flex-1">Suggested links ({suggestionCount})</span>
        <Switch
          checked={showSuggestions}
          onCheckedChange={onShowSuggestions}
          aria-label="Show suggested links"
        />
      </label>
      <p className="text-[11px]">Click a line to see why the two nodes are linked.</p>
    </GlassCard>
  );
}
