import { cn } from '@/lib/utils';

/** The nexc logo (same artwork as /favicon.svg). */
export function BrandMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 64 64" fill="none" aria-hidden className={cn('size-8', className)}>
      <defs>
        <linearGradient
          id="nexc-brand"
          x1="0"
          y1="0"
          x2="64"
          y2="64"
          gradientUnits="userSpaceOnUse"
        >
          <stop stopColor="var(--brand)" />
          <stop offset="1" stopColor="var(--brand-2)" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="16" className="fill-foreground/[0.06]" />
      <path
        d="M20 44 L32 20 L44 44"
        stroke="url(#nexc-brand)"
        strokeWidth="4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M20 44 H44"
        stroke="url(#nexc-brand)"
        strokeWidth="4"
        strokeLinecap="round"
        strokeDasharray="3 6"
      />
      <circle cx="32" cy="20" r="6" fill="url(#nexc-brand)" />
      <circle cx="20" cy="44" r="6" fill="url(#nexc-brand)" />
      <circle cx="44" cy="44" r="6" fill="url(#nexc-brand)" />
    </svg>
  );
}
