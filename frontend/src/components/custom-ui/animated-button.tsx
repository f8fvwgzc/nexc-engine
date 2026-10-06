import type { ComponentProps } from 'react';

import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { cn } from '@/lib/utils';

interface AnimatedButtonProps extends ComponentProps<typeof Button> {
  loading?: boolean;
  /** Replaces the label while loading (defaults to the label itself). */
  loadingText?: string;
  /** Solid primary fill for primary calls to action. */
  glow?: boolean;
}

/** shadcn Button with a loading state, press feedback and an optional brand glow. */
export function AnimatedButton({
  loading = false,
  loadingText,
  glow = false,
  disabled,
  className,
  children,
  ...props
}: AnimatedButtonProps) {
  return (
    <Button
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={cn(
        'transition-[transform,background-color,box-shadow,opacity] duration-200 ease-out-soft motion-safe:active:scale-[0.97]',
        glow && 'bg-primary text-primary-foreground shadow-sm hover:bg-primary/90',
        className,
      )}
      {...props}
    >
      {loading ? (
        <>
          <Spinner className="size-4" />
          {loadingText ?? children}
        </>
      ) : (
        children
      )}
    </Button>
  );
}
