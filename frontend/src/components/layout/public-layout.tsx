import { Link, Outlet } from 'react-router-dom';

import { ThemeToggle } from './theme-toggle';
import { BrandMark } from './brand-mark';

/** Centered auth layout with an ambient gradient backdrop. */
export function PublicLayout() {
  return (
    <div className="relative isolate flex min-h-svh flex-col overflow-hidden bg-background">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(60rem_40rem_at_10%_-10%,color-mix(in_oklch,var(--brand)_18%,transparent),transparent),radial-gradient(50rem_30rem_at_110%_110%,color-mix(in_oklch,var(--brand-2)_14%,transparent),transparent)]"
      />
      <header className="flex items-center justify-between px-4 py-4 sm:px-8">
        <Link
          to="/"
          className="flex items-center gap-2 rounded-md font-semibold tracking-tight focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none"
        >
          <BrandMark className="size-7" />
          nexc-engine
        </Link>
        <ThemeToggle />
      </header>
      <main className="flex flex-1 items-center justify-center px-4 pb-16">
        <Outlet />
      </main>
    </div>
  );
}
