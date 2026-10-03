import { Children, type ComponentProps } from 'react';

import { cn } from '@/lib/utils';

interface FadeInProps extends ComponentProps<'div'> {
  delayMs?: number;
  /** `up` slides in from 8px below; `scale` grows from 92%. */
  variant?: 'fade' | 'up' | 'scale';
}

const VARIANT_CLASS = {
  fade: 'motion-safe:animate-fade-in',
  up: 'motion-safe:animate-fade-up',
  scale: 'motion-safe:animate-scale-in',
} as const;

/** CSS-only entrance animation; a no-op under `prefers-reduced-motion`. */
export function FadeIn({ delayMs = 0, variant = 'up', className, style, ...props }: FadeInProps) {
  return (
    <div
      className={cn(VARIANT_CLASS[variant], className)}
      style={delayMs ? { animationDelay: `${delayMs}ms`, ...style } : style}
      {...props}
    />
  );
}

interface StaggerProps extends ComponentProps<'div'> {
  stepMs?: number;
  /** Cap so long lists don't take forever to settle. */
  maxDelayMs?: number;
}

/** Fades children in one after another. */
export function Stagger({
  stepMs = 45,
  maxDelayMs = 400,
  className,
  children,
  ...props
}: StaggerProps) {
  return (
    <div className={className} {...props}>
      {Children.map(children, (child, i) => (
        <div
          className="motion-safe:animate-fade-up"
          style={{ animationDelay: `${Math.min(i * stepMs, maxDelayMs)}ms` }}
        >
          {child}
        </div>
      ))}
    </div>
  );
}
