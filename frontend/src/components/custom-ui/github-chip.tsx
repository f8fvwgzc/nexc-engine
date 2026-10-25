import { githubLabel, githubRef, type GithubLabelForm } from '@/lib/github';
import { cn } from '@/lib/utils';

/** The GitHub mark (lucide has no brand icons). */
export function GithubMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 16 16" aria-hidden className={className}>
      <path
        fill="currentColor"
        d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z"
      />
    </svg>
  );
}

/** How a chip looks: a small outlined pill (rows, cards), or plain text like a property value. */
const VARIANT_CLASS = {
  pill: 'h-5 gap-1 rounded-full border bg-background px-1.5 font-mono text-[11px] hover:border-ring/60',
  plain: 'h-7 gap-2 rounded-md px-1.5 text-[13px] hover:bg-muted',
} as const;

/**
 * The GitHub issue or pull request an issue is linked to: the mark and `owner/repo#12` (shorter
 * with `form`), opening GitHub in a new tab. A click on it never opens the row or card it sits
 * on, and dragging it drags the card.
 */
export function GithubChip({
  url,
  form = 'full',
  variant = 'pill',
  className,
}: {
  url: string;
  form?: GithubLabelForm;
  variant?: keyof typeof VARIANT_CLASS;
  className?: string;
}) {
  const what = githubRef(url)?.kind === 'pull' ? 'pull request' : 'issue';
  return (
    <a
      href={url}
      target="_blank"
      rel="noreferrer"
      title={url}
      draggable={false}
      onClick={(e) => e.stopPropagation()}
      className={cn(
        'relative z-10 inline-flex max-w-full min-w-0 shrink-0 items-center text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50',
        VARIANT_CLASS[variant],
        className,
      )}
    >
      <GithubMark className={cn('shrink-0', variant === 'pill' ? 'size-3' : 'size-3.5')} />
      <span className="sr-only">GitHub {what} </span>
      <span className="truncate">{githubLabel(url, form)}</span>
      <span className="sr-only"> (opens in a new tab)</span>
    </a>
  );
}
