import type { ComponentProps } from 'react';

import { cn } from '@/lib/utils';

interface GlassCardProps extends ComponentProps<'div'> {
  /** Adds a lift + glow on hover (for clickable cards). */
  interactive?: boolean;
}

/** Translucent, blurred surface with a hairline gradient border. */
export function GlassCard({ className, interactive = false, ...props }: GlassCardProps) {
  return (
    <div
      data-slot="glass-card"
      className={cn(
        'relative rounded-xl border border-white/10 bg-card/70 shadow-sm backdrop-blur-xl supports-[backdrop-filter]:bg-card/55',
        'before:pointer-events-none before:absolute before:inset-0 before:rounded-[inherit] before:bg-gradient-to-b before:from-white/[0.06] before:to-transparent',
        'dark:border-white/[0.07]',
        interactive &&
          'transition-[transform,box-shadow,border-color] duration-300 ease-out-soft focus-within:border-brand/40 hover:-translate-y-0.5 hover:border-brand/30 hover:shadow-lg hover:shadow-brand/5',
        className,
      )}
      {...props}
    />
  );
}
