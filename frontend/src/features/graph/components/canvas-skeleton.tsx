import { Spinner } from '@/components/custom-ui/spinner';

export function CanvasSkeleton() {
  return (
    <div className="graph-canvas flex size-full items-center justify-center" aria-busy>
      <Spinner label="Loading canvas…" />
    </div>
  );
}
