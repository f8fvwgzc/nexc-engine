import { MousePointerClickIcon, SparklesIcon } from 'lucide-react';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { KbdHint } from '@/components/custom-ui/kbd-hint';

/** Shown over an empty canvas: how to start. */
export function CanvasEmptyHint() {
  return (
    <div className="pointer-events-none absolute inset-0 flex items-center justify-center p-6">
      <GlassCard className="max-w-sm space-y-3 p-5 text-center motion-safe:animate-scale-in">
        <MousePointerClickIcon className="mx-auto size-6 text-brand" aria-hidden />
        <p className="font-medium">This graph is empty</p>
        <p className="text-sm text-muted-foreground">
          Double-click anywhere or press <KbdHint keys="n" className="inline-flex" /> to add a node.
          Or use <SparklesIcon className="inline size-3.5 text-brand" aria-hidden />{' '}
          <span className="font-medium text-foreground">Plan</span> to let the LLM draft one from
          the graph goal.
        </p>
      </GlassCard>
    </div>
  );
}
