import { Spinner as BaseSpinner } from '@/components/ui/spinner';
import { cn } from '@/lib/utils';

/** Centered spinner with an optional label, for inline loading regions. */
export function Spinner({ label, className }: { label?: string; className?: string }) {
  return (
    <div
      className={cn(
        'flex items-center justify-center gap-2 text-sm text-muted-foreground motion-safe:animate-fade-in',
        className,
      )}
    >
      <BaseSpinner className="size-4 text-brand" />
      {label && <span>{label}</span>}
    </div>
  );
}
