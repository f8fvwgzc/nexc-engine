import { useNavigation } from 'react-router-dom';

/** Thin top bar while a route chunk / navigation is loading. */
export function NavigationProgress() {
  const busy = useNavigation().state !== 'idle';
  return (
    <div
      aria-hidden
      className="pointer-events-none fixed inset-x-0 top-0 z-50 h-0.5 overflow-hidden transition-opacity duration-300 data-[busy=false]:opacity-0"
      data-busy={busy}
    >
      <div className="h-full w-full animate-shimmer bg-[linear-gradient(90deg,transparent,var(--brand),var(--brand-2),transparent)] bg-[length:200%_100%]" />
    </div>
  );
}
